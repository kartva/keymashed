use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired};
use std::time::Duration;
use std::net::Ipv4Addr;
use crate::{rtp, udp_connect_retry, RECV_AUDIO_PORT, RECV_IP, SEND_IP, SEND_AUDIO_PORT};

pub const AUDIO_SAMPLE_COUNT: usize = 1024;
pub const AUDIO_FREQUENCY: i32 = 44100;
pub const AUDIO_BUFFER_LENGTH: usize = 1024;

pub struct AudioCallbackData {
    last: [f32; AUDIO_SAMPLE_COUNT],
    recv: rtp::RtpSizedPayloadReceiver<[f32; AUDIO_SAMPLE_COUNT], AUDIO_BUFFER_LENGTH>,
}

impl AudioCallback for AudioCallbackData {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        let mut locked_receiver = self.recv.lock_receiver();

        // If the circular buffer hasn't seen enough future packets, wait for more to arrive
        // Handles the case: sender is falling behind in sending packets.
        while locked_receiver.early_latest_span() < 5 {
            log::debug!("Sleeping and waiting for more packets to arrive. Early-latest span {}", locked_receiver.early_latest_span());
            drop(locked_receiver);
            std::thread::sleep(Duration::from_millis(
                (1000 * AUDIO_SAMPLE_COUNT as u64) / (AUDIO_FREQUENCY as u64),
            ));
            locked_receiver = self.recv.lock_receiver();
        }

        let received_packet = locked_receiver.consume_earliest_packet();

        if let Some(packet) = received_packet.get_data() {
            log::info!("Playing packet with seq: {:?}", packet.header);

            out.copy_from_slice(&packet.data);

            self.last = packet.data;
        } else {
            log::info!("No packet to play. Playing last received packet again.");
        }
    }
}

/// Start playing audio from a UDP stream. Audio will play until returned device is dropped.
/// Ensure that the frequency, sample count and bit depth of the sender and receiver match.

pub fn play_audio(audio_subsystem: &sdl2::AudioSubsystem) -> AudioDevice<AudioCallbackData> {
    let sock = udp_connect_retry((Ipv4Addr::UNSPECIFIED, RECV_AUDIO_PORT));
    sock.connect((SEND_IP, SEND_AUDIO_PORT)).unwrap();

    let recv: rtp::RtpSizedPayloadReceiver<[f32; AUDIO_SAMPLE_COUNT], AUDIO_BUFFER_LENGTH> = rtp::RtpReceiver::new(sock);

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

/// Start sending audio over a UDP stream. Audio will be sent indefinitely.
pub fn send_audio() -> ! {
    let sock = udp_connect_retry((Ipv4Addr::UNSPECIFIED, SEND_AUDIO_PORT));
    sock.connect((RECV_IP, RECV_AUDIO_PORT)).unwrap();
    let mut sender: rtp::RtpSizedPayloadSender<[f32; AUDIO_SAMPLE_COUNT]> = rtp::RtpSizedPayloadSender::new(sock);
    
    let mut time = 0.0;
    let mut audio_wav_reader = std::iter::from_fn(move || {
        time += 1.0 / AUDIO_FREQUENCY as f32;
        Some(0.5 * (2.0 * std::f32::consts::PI * 440.0 * time).sin())
    });

    log::info!("Starting to send audio!");

    loop {
        sender.send(|bytes: &mut [f32; AUDIO_SAMPLE_COUNT]| {
            for idx in 0..AUDIO_SAMPLE_COUNT {
                bytes[idx] = audio_wav_reader.next().unwrap();
            }
        });
        std::thread::sleep(Duration::from_millis(
            (1000 * AUDIO_SAMPLE_COUNT as u64) / (AUDIO_FREQUENCY as u64),
        ));
        log::trace!("Sent audio packet.");
    }
}