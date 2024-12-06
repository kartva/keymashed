#![feature(generic_const_exprs)]

mod bpf;
mod cli;
mod rtp;
mod audio;
mod video;

use sdl2::{self, pixels::PixelFormatEnum};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};
use std::{net::UdpSocket, time::Duration};

use simplelog::WriteLogger;

const VIDEO_WIDTH: u32 = 300;
const VIDEO_HEIGHT: u32 = 150;
const VIDEO_FPS_TARGET: f64 = 24.0;

const ROW_WIDTH: usize = (VIDEO_WIDTH as usize) * 3;

const AUDIO_SEND_ADDR: &str = "127.0.0.1:44443";
const AUDIO_DEST_ADDR: &str = "127.0.0.1:44406";
const VIDEO_SEND_ADDR: &str = "127.0.0.1:44001";
const VIDEO_DEST_ADDR: &str = "127.0.0.1:44002";

#[derive(FromBytes, Debug, IntoBytes, Immutable, KnownLayout)]
struct VideoPacket {
    frame_num: u32,
    frame_data: [u8; 900]
}

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
    let video_reciever = rtp::RtpReciever::<VideoPacket, 8192>::new(video_recieving_socket);

    std::thread::spawn(move || {
        send_video();
    });

    // let packets queue up
    std::thread::sleep(Duration::from_secs(2));

    let mut frame_count = 0;
    loop {
        let start_time = std::time::Instant::now();

        let mut event_pump = sdl_context.event_pump().unwrap();
        for event in event_pump.poll_iter() {
            match event {
                sdl2::event::Event::Quit {..} => return Ok(()),
                _ => {}
            }
        }

        texture.with_lock(None, |buffer: &mut [u8], _pitch: usize| {
            log::info!("Playing frame {} (seq {} - {})", frame_count , frame_count * VIDEO_HEIGHT as u32, (frame_count + 1) * VIDEO_HEIGHT as u32);
            
            let mut row_index = 0usize;
            let mut locked_video_reciever = video_reciever.lock_reciever();

            // If the circular buffer hasn't seen enough future packets, wait for more to arrive
            // Handles the case: sender is falling behind in sending packets.
            while locked_video_reciever.early_latest_span() < VIDEO_HEIGHT {
                drop(locked_video_reciever);
                std::thread::sleep(Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET));
                locked_video_reciever = video_reciever.lock_reciever();
            }

            while (row_index as u32) < VIDEO_HEIGHT {
                let chunk = &mut buffer[row_index * ROW_WIDTH..(row_index + 1) * ROW_WIDTH];
                // if we have a packet with a higher frame number, earlier packets have been dropped from the circular buffer
                // so redraw the current frame with more up-to-date packets (and skip ahead to a later frame)
                // Handles the case: receiver is falling behind in consuming packets.
                
                if let Some(p) = locked_video_reciever.peek_earliest_packet() {
                    if p.data.frame_num > frame_count {
                        log::info!("Skipping ahead to frame {}", p.data.frame_num);
                        row_index = 0;
                    }
                }

                let packet = locked_video_reciever.consume_earliest_packet();
                if let Some(packet) = packet.get_data() {
                    chunk.copy_from_slice(packet.data.frame_data.as_ref());
                } else {
                    chunk.iter_mut().for_each(|x| *x = 0); // blacks out the row; useful for visually observing packet loss
                }
                row_index += 1;
            }
            frame_count += 1;
        }).unwrap();

        renderer.clear();
        renderer.copy(&texture, None, None).unwrap();
        renderer.present();

        let elapsed = start_time.elapsed();
        // delay to hit target FPS
        if elapsed < Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET) {
            std::thread::sleep(Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET) - elapsed);
        }
    }
}

pub fn send_video() {
    let sock = std::net::UdpSocket::bind(VIDEO_SEND_ADDR).unwrap();

    sock.connect(VIDEO_DEST_ADDR).unwrap();
    let mut sender = rtp::RtpSender::new(sock);
    log::info!("Starting to send video!");

    let mut frame = [0u8; (VIDEO_WIDTH as usize) * (VIDEO_HEIGHT as usize) * 3];
    assert!(frame.len() % VIDEO_WIDTH as usize == 0);

    let mut frame_counter = 0;
    loop {
        let start_time = std::time::Instant::now();

        for y in 0..VIDEO_HEIGHT {
            for x in 0..VIDEO_WIDTH {
                let offset = (y as usize) * (VIDEO_WIDTH as usize) * 3 + (x as usize) * 3;
                frame[offset] = ((x + frame_counter) % 256) as u8; // Red channel
                frame[offset + 1] = ((y + frame_counter * 2) % 256) as u8; // Green channel
                frame[offset + 2] = ((x + y + frame_counter * 3) % 256) as u8; // Blue channel
            }
        }

        for chunk in frame.chunks_exact(VIDEO_WIDTH as usize * 3) { // send one-third row at a time
            let chunk = VideoPacket {
                frame_num: frame_counter as u32,
                frame_data: chunk.try_into().unwrap()
            };
            sender.send(chunk.as_bytes());
        }

        log::info!("Sent frame {} (seq {} - {})", frame_counter, frame_counter * VIDEO_HEIGHT as u32, (frame_counter + 1) * VIDEO_HEIGHT as u32);

        let elapsed = start_time.elapsed();
        // delay to hit target FPS
        if elapsed < Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET) {
            std::thread::sleep(Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET) - elapsed);
        }
        frame_counter += 1;
    }
}