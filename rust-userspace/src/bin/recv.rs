#![feature(generic_const_exprs)]

use rand::Rng;
use run_louder::*;

use bytes::Buf;
use sdl2::{self, pixels::{Color, PixelFormatEnum}, rect::Rect};
use video::{decode_quantized_macroblock, dequantize_macroblock, Macroblock, MutableYUVFrame};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};
use std::{io::Write, net::UdpSocket, time::Duration};

use simplelog::WriteLogger;

#[derive(FromBytes, Debug, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
struct VideoPacket {
    data: [u8; 1504]
}

fn main() -> std::io::Result<()> {
    let log_file: Box<dyn Write + Send> = if BUFFER_LOGS {
        Box::new(std::io::BufWriter::with_capacity(
            65536 /* 64 KiB */,
            std::fs::File::create("recv.log")?
        ))
    } else {
        Box::new(std::fs::File::create("recv.log")?)
    };

    WriteLogger::init(
        LOG_LEVEL,
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

    let display_mode = video_subsystem.desktop_display_mode(0).unwrap();

    let window = video_subsystem.window("rust-userspace", display_mode.w as u32, display_mode.h as u32)
        .position_centered().fullscreen_desktop()
        .build().unwrap();
    // we don't use vsync here because my monitor runs at 120hz and I don't want to stream at that rate

    let mut renderer = window.into_canvas().accelerated().build().unwrap();

    // Get the window's current size
    let (window_width, window_height) = renderer.output_size().unwrap();

    // Calculate the position to center the texture at its original resolution
    let x = (window_width - VIDEO_WIDTH) / 2;
    let y = (window_height - VIDEO_HEIGHT) / 2;

    // Create a destination rectangle for the texture at its original size
    let dest_rect = Rect::new(x as i32, y as i32, VIDEO_WIDTH, VIDEO_HEIGHT);


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

        renderer.set_draw_color(Color::CYAN);
        renderer.clear();

        texture.with_lock(None, |buffer: &mut [u8], _pitch: usize| {
            log::info!("Playing frame {}", frame_count);
            
            let mut packet_index = 0usize;
            let mut locked_video_reciever = video_reciever.lock_reciever();

            // If the circular buffer hasn't seen enough future packets, wait for more to arrive
            // Handles the case: sender is falling behind in sending packets.
            while locked_video_reciever.early_latest_span() < 5 {
                log::debug!("Sleeping and waiting for more packets to arrive. Early-latest span {}", locked_video_reciever.early_latest_span());
                return;
            }

            const BLOCK_WRITTEN_WIDTH: usize = (VIDEO_WIDTH as usize) / PACKET_X_DIM;
            const BLOCK_WRITTEN_HEIGHT: usize = (VIDEO_HEIGHT as usize) / PACKET_Y_DIM;

            let mut block_written = [[false; BLOCK_WRITTEN_WIDTH]; BLOCK_WRITTEN_HEIGHT];

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
                    loop {
                        let cursor_position = cursor_start_len - cursor.remaining();
                        let x = cursor.get_u16() as usize;
                        let y = cursor.get_u16() as usize;
                        let quality = cursor.get_f64();

                        if (x == u16::MAX as usize) && (y == u16::MAX as usize) {
                            break;
                        }

                        block_written[y / PACKET_Y_DIM][x / PACKET_X_DIM] = true;

                        log::trace!("Receiving MacroblockWithPos at ({frame_count}, {x}, {y}) at cursor position {cursor_position}");

                        let decoded_quantized_macroblock;
                        (decoded_quantized_macroblock, cursor) = decode_quantized_macroblock(&cursor);
                        let macroblock = dequantize_macroblock(&decoded_quantized_macroblock, quality);
                        macroblock.copy_to_yuv422_frame(MutableYUVFrame::new(VIDEO_WIDTH as usize, VIDEO_HEIGHT as usize, buffer), x, y);
                    }
                }
                packet_index += 1;
            }

            // Committed for future use:
            // // write pretty noise to the blocks that weren't written to
            // for block_y in 0..BLOCK_WRITTEN_HEIGHT {
            //     for block_x in 0..BLOCK_WRITTEN_WIDTH {
            //         if !block_written[block_y][block_x] {
            //             let mut rng = rand::thread_rng();
            //             for packet_y in 0..PACKET_Y_DIM {
            //                 for packet_x in 0..PACKET_X_DIM {
            //                     let y = block_y * PACKET_Y_DIM + packet_y;
            //                     let x = block_x * PACKET_X_DIM + packet_x;
            //                     let noise = rng.gen_range(-32..32);
            //                     let luminance_idx = y * VIDEO_WIDTH as usize * PIXEL_WIDTH + x * PIXEL_WIDTH as usize;
            //                     let cr_or_cb_idx = y + 1;
                                
            //                     buffer[luminance_idx] = buffer[luminance_idx].wrapping_add_signed(noise);
            //                     buffer[cr_or_cb_idx] = buffer[cr_or_cb_idx].wrapping_add_signed(noise);
            //                 }
            //             }        
            //         }
            //     }
            // }

            frame_count += 1;
        }).unwrap();

        renderer.copy(&texture, None, dest_rect).unwrap();
        renderer.present();

        let elapsed = start_time.elapsed();
        // delay to hit target FPS
        let target_latency = Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET);
        if elapsed < target_latency {
            std::thread::sleep(target_latency - elapsed);
        } else {
            log::warn!("Receiver took too long presenting; overshot frame deadline by {} ms", (elapsed - target_latency).as_millis());
        }
    }
}