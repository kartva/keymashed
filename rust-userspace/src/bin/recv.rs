use run_louder::*;

use bytes::{Buf, BufMut, Bytes};
use run_louder::rtp::RtpSender;
use sdl2::{self, pixels::PixelFormatEnum};
use video::{decode_quantized_macroblock, dequantize_macroblock, encode_quantized_macroblock, quantize_macroblock, MacroblockWithPosition, MutableYUVFrame, YUVFrame, YUVFrameMacroblockIterator};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, TryFromBytes};
use std::{net::UdpSocket, time::Duration};

use simplelog::WriteLogger;

#[derive(FromBytes, Debug, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
struct VideoPacket {
    data: [u8; 1504]
}

fn main() -> std::io::Result<()> {
    let log_file = std::io::BufWriter::with_capacity(
        65536 /* 64 KiB */,
        std::fs::File::create("recv.log")?
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

    // let audio_subsystem = sdl_context.audio().unwrap();
    // let _audio = audio::play_audio(&audio_subsystem);

    let window = video_subsystem.window("rust-userspace", VIDEO_WIDTH, VIDEO_HEIGHT)
        .position_centered()
        .build().unwrap();

    // we don't use vsync here because my monitor runs at 120hz and I don't want to stream at that rate

    let mut renderer = window.into_canvas().accelerated().build().unwrap();

    let texture_creator = renderer.texture_creator();
    let mut texture = texture_creator.create_texture_streaming(PixelFormatEnum::YUY2, VIDEO_WIDTH, VIDEO_HEIGHT).unwrap();

    let video_recieving_socket = UdpSocket::bind(VIDEO_DEST_ADDR).unwrap();
    let video_reciever = rtp::RtpReciever::<VideoPacket, 8192>::new(video_recieving_socket);

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