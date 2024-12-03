#![feature(generic_const_exprs)]

mod bpf;
mod cli;
mod rtp;
mod audio;

use sdl2::{self, pixels::PixelFormatEnum};
use std::{net::UdpSocket, time::Duration};

use simplelog::WriteLogger;

const VIDEO_WIDTH: u32 = 200;
const VIDEO_HEIGHT: u32 = 100;
const VIDEO_FPS_TARGET: f64 = 6.0;

const AUDIO_SEND_ADDR: &str = "127.0.0.1:44443";
const AUDIO_DEST_ADDR: &str = "127.0.0.1:44406";
const VIDEO_SEND_ADDR: &str = "127.0.0.1:44001";
const VIDEO_DEST_ADDR: &str = "127.0.0.1:44002";

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
    let video_subsystem = sdl_context.video().unwrap();
    let audio_subsystem = sdl_context.audio().unwrap();
    let timer_subsystem = sdl_context.timer().unwrap();

    std::thread::spawn(move || {
        // audio::send_audio();
    });

    // Keep this handle alive to keep the audio running.
    // let _device = audio::play_audio(&audio_subsystem);

    let window = video_subsystem.window("rust-userspace", VIDEO_WIDTH, VIDEO_HEIGHT)
        .position_centered()
        .build().unwrap();

    // we don't use vsync here because my monitor runs at 120hz and I don't want to stream at that rate

    let mut renderer = window.into_canvas().accelerated().build().unwrap();

    let texture_creator = renderer.texture_creator();
    let mut texture = texture_creator.create_texture_streaming(PixelFormatEnum::RGB24, VIDEO_WIDTH, VIDEO_HEIGHT).unwrap();

    let video_recieving_socket = UdpSocket::bind(VIDEO_DEST_ADDR).unwrap();
    let video_reciever = rtp::RtpReciever::<[u8; VIDEO_WIDTH as usize], 8192>::new(video_recieving_socket);

    std::thread::spawn(move || {
        send_video();
    });

    // let packets queue up
    std::thread::sleep(Duration::from_secs(2));

    let mut time = 0;
    loop {
        let frame_start_count = timer_subsystem.performance_counter();

        let mut event_pump = sdl_context.event_pump().unwrap();
        for event in event_pump.poll_iter() {
            match event {
                sdl2::event::Event::Quit {..} => return Ok(()),
                _ => {}
            }
        }

        texture.with_lock(None, |buffer: &mut [u8], _pitch: usize| {
            time += 1;
            for chunk in buffer.chunks_exact_mut(VIDEO_WIDTH as usize) {
                let mut locked_video_reciever = video_reciever.lock_reciever_for_consumption();
                let packet = locked_video_reciever.consume_earliest_packet();
                if let Some(packet) = packet.get_data() {
                    chunk.copy_from_slice(packet.data.as_ref());
                }
            }
        }).unwrap();

        renderer.clear();
        renderer.copy(&texture, None, None).unwrap();
        renderer.present();

        let frame_end_count = timer_subsystem.performance_counter();
        let elapsed = (frame_end_count - frame_start_count) as f64 / timer_subsystem.performance_frequency() as f64;
        // delay to hit target FPS
        if elapsed < 1.0 / VIDEO_FPS_TARGET {
            std::thread::sleep(Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET - elapsed));
        }
    }
}

pub fn send_video() {
    let sock = std::net::UdpSocket::bind(VIDEO_SEND_ADDR).unwrap();

    sock.connect(VIDEO_DEST_ADDR).unwrap();
    let mut sender = rtp::RtpSender::new(sock);
    log::info!("Starting to send video!");

    let mut frame = [0u8; (VIDEO_WIDTH as usize) * (VIDEO_HEIGHT as usize) * 3];

    let mut time = 0;
    loop {
        let start_time = std::time::Instant::now();

        for y in 0..VIDEO_HEIGHT {
            for x in 0..VIDEO_WIDTH {
                let offset = (y as usize) * (VIDEO_WIDTH as usize) * 3 + (x as usize) * 3;
                frame[offset] = ((x + time) % 256) as u8; // Red channel
                frame[offset + 1] = ((y + time * 2) % 256) as u8; // Green channel
                frame[offset + 2] = ((x + y + time * 3) % 256) as u8; // Blue channel
            }
        }

        time += 3;

        assert!(frame.len() % VIDEO_WIDTH as usize == 0);
        for chunk in frame.chunks_exact(VIDEO_WIDTH as usize) { // send one-third row at a time
            sender.send(chunk);
        }

        let elapsed = start_time.elapsed();
        // delay to hit target FPS
        if elapsed < Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET) {
            std::thread::sleep(Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET) - elapsed);
        }
    }
}