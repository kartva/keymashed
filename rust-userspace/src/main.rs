#![feature(generic_const_exprs)]

mod bpf;
mod cli;
mod rtp;
mod audio;
mod video;

use bytes::{Buf, BufMut, Bytes};
use rtp::RtpSender;
use sdl2::{self, pixels::PixelFormatEnum};
use video::{decode_quantized_macroblock, dequantize_macroblock, encode_quantized_macroblock, quantize_macroblock, MacroblockWithPosition, MutableYUVFrame, YUVFrame, YUVFrameMacroblockIterator};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, TryFromBytes};
use std::{net::UdpSocket, time::Duration};

use simplelog::WriteLogger;

const VIDEO_WIDTH: u32 = 640;
const VIDEO_HEIGHT: u32 = 480;
const VIDEO_FPS_TARGET: f64 = 30.0;

const PACKET_SEND_THRESHOLD: usize = 1500;

const AUDIO_SEND_ADDR: &str = "127.0.0.1:44443";
const AUDIO_DEST_ADDR: &str = "127.0.0.1:44406";
const VIDEO_SEND_ADDR: &str = "127.0.0.1:44001";
const VIDEO_DEST_ADDR: &str = "127.0.0.1:44002";

#[derive(FromBytes, Debug, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
struct VideoPacket {
    data: [u8; 1504]
}

// Keep this in sync with frame_data size. Putting this directly in the above struct causes a compiler cyclic error due to IntoBytes.
const PIXEL_WIDTH: usize = 2;
const PACKET_X_DIM: usize = 16;
const PACKET_Y_DIM: usize = 16;
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
    std::thread::sleep(Duration::from_secs(3));

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
            while locked_video_reciever.early_latest_span() < 5 {
                log::debug!("Sleeping and waiting for more packets to arrive. Early-latest span {}", locked_video_reciever.early_latest_span());
                drop(locked_video_reciever);
                std::thread::sleep(Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET));
                locked_video_reciever = video_reciever.lock_reciever();
            }

            while (packet_index as u32) < (VIDEO_HEIGHT * VIDEO_WIDTH * PIXEL_WIDTH as u32 / PACKET_SIZE as u32) {
                // if we have a packet with a higher frame number, earlier packets have been dropped from the circular buffer
                // so redraw the current frame with more up-to-date packets (and skip ahead to a later frame)
                // Handles the case: receiver is falling behind in consuming packets.
                log::trace!("Playing Frame {frame_count} packet index: {}", packet_index);
                
                if let Some(p) = locked_video_reciever.peek_earliest_packet() {
                    let mut cursor = &p.data.data[..];
                    let packet_frame_count = cursor.get_u32();
                    if packet_frame_count > frame_count {
                        log::info!("Skipping ahead to frame {}", packet_frame_count);
                        frame_count = packet_frame_count;
                        packet_index = 0;
                    }
                }

                let packet = locked_video_reciever.consume_earliest_packet();
                if let Some(packet) = packet.get_data() {
                    // copy the packet data into the buffer
                    let mut cursor = &packet.data.data[..];

                    let cursor_start_len = cursor.len();
                    let _packet_frame_count = cursor.get_u32();
                    while cursor.has_remaining() {
                        let x = cursor.get_u16() as usize;
                        let y = cursor.get_u16() as usize;

                        let decoded_quantized_macroblock;
                        log::trace!("Receiving macroblock at ({}, {}, {}) at cursor position {}", frame_count, x, y, cursor_start_len - cursor.remaining());
                        (decoded_quantized_macroblock, cursor) = decode_quantized_macroblock(&cursor);
                        let macroblock = dequantize_macroblock(&decoded_quantized_macroblock);
                        macroblock.copy_to_yuv422_frame(MutableYUVFrame::new(VIDEO_WIDTH as usize, VIDEO_HEIGHT as usize, buffer), x, y);
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
    let mut sender: RtpSender<[u8]> = rtp::RtpSender::new(sock);
    log::info!("Starting to send video!");

    let mut camera = rscam::Camera::new("/dev/video1").unwrap();
    camera.start(&rscam::Config {
        interval: (1, 30),
        resolution: (VIDEO_WIDTH as _, VIDEO_HEIGHT as _),
        format: b"YUYV",
        ..Default::default()
    }).unwrap();

    let mut frame_count = 0;
    loop {
        let frame = camera.capture().unwrap();
        let frame: &[u8] = frame.as_ref();
        assert!(frame.len() % (VIDEO_WIDTH * PIXEL_WIDTH as u32) as usize == 0);
        assert!(frame.len() / (VIDEO_WIDTH as usize * PIXEL_WIDTH) == VIDEO_HEIGHT as usize);

        let start_time = std::time::Instant::now();

        let frame = YUVFrame::new(VIDEO_WIDTH as usize, VIDEO_HEIGHT as usize, frame);
        let mut packet_buf = Vec::new();

        packet_buf.put_u32(frame_count);
        for MacroblockWithPosition {x, y, block} in YUVFrameMacroblockIterator::new(&frame) {
            let quantized_macroblock = quantize_macroblock(&block);

            let mut mb_buf = Vec::new();
            mb_buf.put_u16(x as u16);
            mb_buf.put_u16(y as u16);
            encode_quantized_macroblock(&quantized_macroblock, &mut mb_buf);

            if packet_buf.len() + mb_buf.len() >= PACKET_SEND_THRESHOLD {
                // send the packet and start a new one
                sender.send(&packet_buf);
                packet_buf.clear();
                packet_buf.put_u32(frame_count);
            }

            log::trace!("Storing macroblock at ({}, {}, {}) at cursor position {}", frame_count, x, y, packet_buf.len());
            packet_buf.put_slice(&mb_buf);
        }

        log::info!("Sent frame {}", frame_count);

        let elapsed = start_time.elapsed();
        // delay to hit target FPS
        if elapsed < Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET) {
            std::thread::sleep(Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET) - elapsed);
        }
        frame_count += 1;
    }
}