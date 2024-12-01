#![feature(generic_const_exprs)]

mod bpf;
mod cli;
mod rtp;

use sdl2::audio::{AudioCallback, AudioSpecDesired};
use sdl2::{self};
use std::time::Duration;

use simplelog::WriteLogger;

fn main() -> std::io::Result<()> {
    let log_file = std::fs::File::create("rust-userspace.log")?;

    WriteLogger::init(
        log::LevelFilter::Trace,
        simplelog::Config::default(),
        log_file,
    )
    .unwrap();

    // std::thread::spawn(move || {
    //     log::info!("Starting BPF thread");
    //     let bpf_handle = unsafe { bpf::init().unwrap() };
    //     log::info!("BPF map found and opened");
    //     loop {
    //         match receiver.recv() {
    //             Ok(val) => bpf_handle.write_to_map(0, val).unwrap(),
    //             Err(_) => break,
    //         }
    //     }
    // });
    // cli::main(sender)

    let sdl_context = sdl2::init().unwrap();
    let audio_subsystem = sdl_context.audio().unwrap();

    // let video_subsystem = sdl_context.video().unwrap();
    // let window = video_subsystem.window("rust-userspace", 800, 600)
    //     .position_centered()
    //     .build().unwrap();
    // let renderer = window.into_canvas().present_vsync().accelerated().build().unwrap();

    // let texture_creator = renderer.texture_creator();
    // let texture = texture_creator.create_texture_streaming(PixelFormatEnum::RGB888, width, height).unwrap();

    std::thread::spawn(move || {
        send_audio();
    });

    play_audio(audio_subsystem);
    Ok(())
}

pub const AUDIO_SAMPLE_COUNT: usize = 1024;

struct AudioCallbackData {
    last: [f32; AUDIO_SAMPLE_COUNT],
    recv: rtp::RtpReciever<[f32; AUDIO_SAMPLE_COUNT]>,
}

impl AudioCallback for AudioCallbackData {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        let mut locked_reciever = self.recv.lock_reciever_for_consumption();
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

pub fn play_audio(audio_subsystem: sdl2::AudioSubsystem) {
    let sock = std::net::UdpSocket::bind("127.0.0.1:44002").unwrap();
    let recv: rtp::RtpReciever<[f32; AUDIO_SAMPLE_COUNT]> = rtp::RtpReciever::new(sock);

    let desired_spec = AudioSpecDesired {
        freq: Some(44100),
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

    // let packets queue up
    std::thread::sleep(Duration::from_secs(1));

    device.resume();

    // play for 10 seconds
    std::thread::sleep(Duration::from_secs(10));
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

pub fn send_audio() {
    let sock = std::net::UdpSocket::bind("127.0.0.1:44001").unwrap();
    sock.connect("127.0.0.1:44002").unwrap();
    let mut sender = rtp::RtpSender::new(sock);

    let mut square_wave = SquareWave::new(44100.0);
    let mut bytes = [0.0; AUDIO_SAMPLE_COUNT];

    loop {
        square_wave.step(&mut bytes);
        sender.send(bytes);
        std::thread::sleep(Duration::from_millis(
            ((1000 * AUDIO_SAMPLE_COUNT) / 44100) as _,
        ));
    }
}
