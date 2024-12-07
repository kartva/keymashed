use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired};
use std::time::Duration;
use std::net::{Ipv4Addr, UdpSocket};
use crate::rtp;

use crate::{AUDIO_DEST_ADDR, AUDIO_SEND_ADDR};

pub const AUDIO_SAMPLE_COUNT: usize = 1024;
pub const AUDIO_FREQUENCY: i32 = 44100;
pub const AUDIO_BUFFER_LENGTH: usize = 1024;

pub struct AudioCallbackData {
    last: [f32; AUDIO_SAMPLE_COUNT],
    recv: rtp::RtpReciever<[f32; AUDIO_SAMPLE_COUNT], AUDIO_BUFFER_LENGTH>,
}

impl AudioCallback for AudioCallbackData {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        let mut locked_reciever = self.recv.lock_reciever();

        // If the circular buffer hasn't seen enough future packets, wait for more to arrive
        // Handles the case: sender is falling behind in sending packets.
        while locked_reciever.early_latest_span() < 5 {
            log::debug!("Sleeping and waiting for more packets to arrive. Early-latest span {}", locked_reciever.early_latest_span());
            drop(locked_reciever);
            std::thread::sleep(Duration::from_millis(
                (1000 * AUDIO_SAMPLE_COUNT as u64) / (AUDIO_FREQUENCY as u64),
            ));
            locked_reciever = self.recv.lock_reciever();
        }

        let recieved_packet = locked_reciever.consume_earliest_packet();

        if let Some(packet) = recieved_packet.get_data() {
            log::info!("Playing packet with seq: {}", packet.sequence_number);

            out.copy_from_slice(&packet.data);

            self.last = packet.data;
        } else {
            log::info!("No packet to play. Playing last received packet again.");
        }
    }
}

/// Start playing audio from a UDP stream. Audio will play until returned device is dropped.
/// Ensure that the frequency, sample count and bit depth of the sender and reciever match.

pub fn play_audio(audio_subsystem: &sdl2::AudioSubsystem) -> AudioDevice<AudioCallbackData> {
    log::info!("Binding to audio destination address: {}", AUDIO_DEST_ADDR);
    let sock = UdpSocket::bind(AUDIO_DEST_ADDR).unwrap();
    let recv: rtp::RtpReciever<[f32; AUDIO_SAMPLE_COUNT], AUDIO_BUFFER_LENGTH> = rtp::RtpReciever::new(sock);

    let desired_spec = AudioSpecDesired {
        freq: Some(AUDIO_FREQUENCY),
        // mono
        channels: Some(1),
        // number of samples
        // should be the same as the number of samples in a packet
        samples: Some(AUDIO_SAMPLE_COUNT as u16),
    };

    let device = audio_subsystem
        .open_playback(None, &desired_spec, |_spec| {
            // initialize the audio callback
            AudioCallbackData {
                last: [0.0; AUDIO_SAMPLE_COUNT],
                recv,
            }
        })
        .unwrap();

    log::info!("Starting to play audio; waiting for packets to queue!");
    // let packets queue up
    std::thread::sleep(Duration::from_secs(1));

    device.resume();
    device
}
struct SquareWave {
    phase_inc: f32,
    phase: f32,
    volume: f32,
}

impl SquareWave {
    fn new(freq: f32) -> Self {
        SquareWave {
            phase_inc: 440.0 / freq,
            phase: 0.0,
            volume: 0.25,
        }
    }

    fn step(&mut self, buf: &mut [f32; AUDIO_SAMPLE_COUNT]) {
        // Generate a square wave
        for x in buf.iter_mut() {
            *x = if self.phase <= 0.5 {
                self.volume
            } else {
                -self.volume
            };
            self.phase = (self.phase + self.phase_inc) % 1.0;
        }
    }
}

/// Start sending audio over a UDP stream. Audio will be sent indefinitely.
pub fn send_audio() -> ! {
    log::info!("Binding to audio send address: {}", AUDIO_SEND_ADDR);
    let sock = UdpSocket::bind("0.0.0.0:44406").unwrap();
    log::info!("Connecting to audio destination address: {}", AUDIO_DEST_ADDR);
    sock.connect(AUDIO_DEST_ADDR).unwrap();
    let mut sender = rtp::RtpSender::new(sock);

    let mut square_wave = SquareWave::new(AUDIO_FREQUENCY as _);
    let mut bytes = [0.0; AUDIO_SAMPLE_COUNT];

    log::info!("Starting to send audio!");

    loop {
        square_wave.step(&mut bytes);
        sender.send(bytes);
        std::thread::sleep(Duration::from_millis(
            (1000 * AUDIO_SAMPLE_COUNT as u64) / (AUDIO_FREQUENCY as u64),
        ));
        log::trace!("Sent audio packet.");
    }
}