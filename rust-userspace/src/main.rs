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

const VIDEO_WIDTH: u32 = 640;
const VIDEO_HEIGHT: u32 = 480;
const VIDEO_FPS_TARGET: f64 = 30.0;

const AUDIO_SEND_ADDR: &str = "127.0.0.1:44443";
const AUDIO_DEST_ADDR: &str = "127.0.0.1:44406";
const VIDEO_SEND_ADDR: &str = "127.0.0.1:44001";
const VIDEO_DEST_ADDR: &str = "127.0.0.1:44002";

#[derive(FromBytes, Debug, IntoBytes, Immutable, KnownLayout)]
struct VideoPacket {
    frame_num: u32,
    x: u32,
    y: u32,
    block: [u8; 1280]
}

// Keep this in sync with frame_data size. Putting this directly in the above struct causes a compiler cyclic error due to IntoBytes.
const PIXEL_WIDTH: usize = 2;
const PACKET_X_DIM: usize = 32;
const PACKET_Y_DIM: usize = 20;
const PACKET_SIZE: usize = PACKET_X_DIM * PACKET_Y_DIM * PIXEL_WIDTH;

fn main() -> std::io::Result<()> {
    let log_file = std::io::BufWriter::with_capacity(
        65536 /* 64 KiB */,
        std::fs::File::create("rust-userspace.log")?
    );

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
    let mut texture = texture_creator.create_texture_streaming(PixelFormatEnum::YUY2, VIDEO_WIDTH, VIDEO_HEIGHT).unwrap();

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
            log::info!("Playing frame {}", frame_count);
            
            let mut packet_index = 0usize;
            let mut locked_video_reciever = video_reciever.lock_reciever();

            // If the circular buffer hasn't seen enough future packets, wait for more to arrive
            // Handles the case: sender is falling behind in sending packets.
            while locked_video_reciever.early_latest_span() < VIDEO_HEIGHT {
                drop(locked_video_reciever);
                std::thread::sleep(Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET));
                locked_video_reciever = video_reciever.lock_reciever();
            }

            while (packet_index as u32) < (VIDEO_HEIGHT * VIDEO_WIDTH * PIXEL_WIDTH as u32 / PACKET_SIZE as u32) {
                // if we have a packet with a higher frame number, earlier packets have been dropped from the circular buffer
                // so redraw the current frame with more up-to-date packets (and skip ahead to a later frame)
                // Handles the case: receiver is falling behind in consuming packets.
                
                if let Some(p) = locked_video_reciever.peek_earliest_packet() {
                    if p.data.frame_num > frame_count {
                        log::info!("Skipping ahead to frame {}", p.data.frame_num);
                        frame_count = p.data.frame_num;
                        packet_index = 0;
                    }
                }

                let packet = locked_video_reciever.consume_earliest_packet();
                if let Some(packet) = packet.get_data() {
                    // copy the packet data into the buffer
                    let x = packet.data.x as usize;
                    let y = packet.data.y as usize;
                    let packet_data_block = &packet.data.block;

                    for i in 0..PACKET_Y_DIM {
                        for j in (0..PACKET_X_DIM).step_by(PIXEL_WIDTH) {
                            let renderbuffer_xy_index = (y + i) * (VIDEO_WIDTH as usize) * PIXEL_WIDTH + (x + j) * PIXEL_WIDTH;
                            let packet_index = i * PACKET_X_DIM + j;
                            for k in 0..PIXEL_WIDTH {
                                buffer[renderbuffer_xy_index + k] = packet_data_block[packet_index + k];
                            }
                        }
                    }
                } else {
                    // chunk.iter_mut().for_each(|x| *x = 0); // blacks out the row; useful for visually observing packet loss
                }
                packet_index += 1;
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

    let mut camera = rscam::Camera::new("/dev/video1").unwrap();
    camera.start(&rscam::Config {
        interval: (1, 30),
        resolution: (VIDEO_WIDTH as _, VIDEO_HEIGHT as _),
        format: b"YUYV",
        ..Default::default()
    }).unwrap();

    // let frame = YUVFrame::new(VIDEO_WIDTH, VIDEO_HEIGHT, frame);

    let mut frame_counter = 0;
    loop {
        let frame = camera.capture().unwrap();
        let frame: &[u8] = frame.as_ref();
        assert!(frame.len() % (VIDEO_WIDTH * PIXEL_WIDTH as u32) as usize == 0);
        assert!(frame.len() / (VIDEO_WIDTH as usize * PIXEL_WIDTH) == VIDEO_HEIGHT as usize);

        let start_time = std::time::Instant::now();

        for y in (0..VIDEO_HEIGHT).step_by(PACKET_Y_DIM) {
            for x in (0..VIDEO_WIDTH).step_by(PACKET_X_DIM) {
                let y = y as usize;
                let x = x as usize;

                let mut packet_data_block = [0u8; PACKET_SIZE];

                for i in 0..PACKET_Y_DIM {
                    for j in (0..PACKET_X_DIM).step_by(PIXEL_WIDTH) {
                        let cambuffer_xy_index = (y + i) * VIDEO_WIDTH as usize * PIXEL_WIDTH + (x + j) * PIXEL_WIDTH;
                        let packet_index = i * PACKET_X_DIM + j;
                        for k in 0..PIXEL_WIDTH {
                            packet_data_block[packet_index + k] = frame[cambuffer_xy_index + k];
                        }
                        
                    }
                }

                let packet = VideoPacket {
                    frame_num: frame_counter,
                    x: x as u32,
                    y: y as u32,
                    block: packet_data_block.try_into().unwrap(),
                };
                sender.send(packet.as_bytes());
            }
        }

        log::info!("Sent frame {}", frame_counter);

        let elapsed = start_time.elapsed();
        // delay to hit target FPS
        if elapsed < Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET) {
            std::thread::sleep(Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET) - elapsed);
        }
        frame_counter += 1;
    }
}