#![feature(generic_const_exprs)]

use run_louder::*;

use bytes::Buf;
use sdl2::{self, pixels::{Color, PixelFormatEnum}, rect::Rect};
use video::{decode_quantized_macroblock, dequantize_macroblock, MutableYUVFrame};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};
use std::{io::Write, net::{Ipv4Addr, UdpSocket}, thread::sleep, time::Duration};

use simplelog::WriteLogger;

#[derive(FromBytes, Debug, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
struct VideoPacket {
    data: [u8; 1504]
}

fn main() -> std::io::Result<()> {
    run_louder::init_logger(false);

    let (bpf_write_channel, bpf_receive_channel) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        log::info!("Starting BPF thread");
        let bpf_handle = unsafe { bpf::init().unwrap() };
        log::info!("BPF map found and opened");
        loop {
            match bpf_receive_channel.recv() {
                Ok(val) => bpf_handle.write_to_map(0, val).unwrap(),
                Err(_) => break,
            }
        }
    });

    // Remove packet loss when setting up network connections.
    bpf_write_channel.send(0).unwrap();

    let sdl_context = sdl2::init().unwrap();

    sdl2::hint::set_video_minimize_on_focus_loss(false);
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

    let video_recieving_socket = udp_connect_retry((Ipv4Addr::UNSPECIFIED, RECV_VIDEO_PORT));
    video_recieving_socket.connect((SEND_IP, SEND_VIDEO_PORT)).unwrap();
    let video_reciever = rtp::RtpSlicePayloadReciever::<u8, PACKET_PAYLOAD_SIZE_THRESHOLD, 8192>::new(video_recieving_socket);

    let sender_communication_socket = udp_connect_retry((Ipv4Addr::UNSPECIFIED, RECV_CONTROL_PORT));
    sender_communication_socket.connect((SEND_IP, SEND_CONTROL_PORT)).unwrap();

    log::info!("Sender connected to control server from {:?}", sender_communication_socket.local_addr().unwrap());

    let mut frame_count = 0;
    let mut typing_metrics = wpm::TypingMetrics::new();
    loop {
        let start_time = std::time::Instant::now();

        // Handle input

        let mut event_pump = sdl_context.event_pump().unwrap();
        for event in event_pump.poll_iter() {
            match event {
                sdl2::event::Event::Quit {..} => return Ok(()),
                sdl2::event::Event::KeyDown { keycode, repeat: false, timestamp: _, .. } => {
                    match keycode {
                        Some(k) => {
                            let ik = k.into_i32();
                            typing_metrics.receive_char_stroke(ik);
                        },
                        _ => {}
                    }
                },
                _ => {}
            }
        }

        let wpm = typing_metrics.calc_wpm();
        log::info!("WPM: {}", wpm);

        let bpf_drop_rate = wpm::wpm_to_drop_amt(wpm);
        log::info!("BPF drop rate: {} ({})", bpf_drop_rate, (bpf_drop_rate as f64 / u32::MAX as f64) * 100.0);

        match bpf_write_channel.send(bpf_drop_rate) {
            Ok(_) => {},
            Err(_) => {
                log::error!("Failed to send BPF drop rate to BPF thread");
            },
        }

        // send desired quality to sender
        let quality = wpm::wpm_to_jpeg_quality(wpm);
        let control_msg = ControlMessage { quality };
        udp_send_retry(&sender_communication_socket, control_msg.as_bytes());
        log::debug!("Sent quality update: {}", quality);

        // Draw video

        renderer.set_draw_color(wpm::wpm_to_sdl_color(wpm, Color::GREEN));
        renderer.clear();

        texture.with_lock(None, |buffer: &mut [u8], _pitch: usize| {            
            let mut locked_video_reciever = video_reciever.lock_reciever();

            // If the circular buffer hasn't seen enough future packets, wait for more to arrive
            // Handles the case: sender is falling behind in sending packets.
            if locked_video_reciever.early_latest_span() < 20 {
                log::info!("Sleeping and waiting for more packets to arrive. Early-latest span {}", locked_video_reciever.early_latest_span());
                return;
            }

            log::info!("Playing frame {}", frame_count);

            const BLOCK_WRITTEN_WIDTH: usize = (VIDEO_WIDTH as usize) / MACROBLOCK_X_DIM;
            const BLOCK_WRITTEN_HEIGHT: usize = (VIDEO_HEIGHT as usize) / MACROBLOCK_Y_DIM;

            let mut block_written = [[false; BLOCK_WRITTEN_WIDTH]; BLOCK_WRITTEN_HEIGHT];
            
            let mut packet_index = 0usize;
            while (packet_index as u32) < (VIDEO_HEIGHT * VIDEO_WIDTH * PIXEL_WIDTH as u32 / MACROBLOCK_BYTE_SIZE as u32) {
                // if we have a packet with a higher frame number, earlier packets have been dropped from the circular buffer
                // so redraw the current frame with more up-to-date packets (and skip ahead to a later frame)
                // Handles the case: receiver is falling behind in consuming packets.
                log::trace!("Playing Frame {frame_count} packet index: {}", packet_index);
                
                if let Some(p) = locked_video_reciever.peek_earliest_packet() {
                    let mut cursor = &p.data[..];
                    let packet_frame_count = cursor.get_u32();
                    if packet_frame_count > frame_count {
                        log::warn!("Skipping ahead to frame {}", packet_frame_count);
                        frame_count = packet_frame_count;
                        packet_index = 0;
                    }
                }

                let packet = locked_video_reciever.consume_earliest_packet();
                if let Some(packet) = packet.get_data() {
                    // copy the packet data into the buffer
                    let mut cursor = &packet.data[..];
                    log::trace!("Packet slice has length {}", cursor.len());

                    let cursor_start_len = cursor.len();
                    let _packet_frame_count = cursor.get_u32();
                    loop {
                        let cursor_position = cursor_start_len - cursor.remaining();
                        let x = cursor.get_u16() as usize;
                        let y = cursor.get_u16() as usize;
                        
                        if (x == u16::MAX as usize) && (y == u16::MAX as usize) {
                            break;
                        }
                        let quality = cursor.get_f64();

                        block_written[y / MACROBLOCK_Y_DIM][x / MACROBLOCK_X_DIM] = true;

                        // log::trace!("Receiving MacroblockWithPos at ({frame_count}, {x}, {y}) at cursor position {cursor_position}");

                        let decoded_quantized_macroblock;
                        (decoded_quantized_macroblock, cursor) = decode_quantized_macroblock(&cursor);
                        let macroblock = dequantize_macroblock(&decoded_quantized_macroblock, quality);
                        macroblock.copy_to_yuv422_frame(MutableYUVFrame::new(VIDEO_WIDTH as usize, VIDEO_HEIGHT as usize, buffer), x, y);
                        packet_index += 1;
                    }
                }
                else {
                    // TODO: fix this hack
                    // roughly 40 macroblocks per packet are packed in
                    packet_index += 40;
                }
            }

            frame_count += 1;
        }).unwrap();

        renderer.copy(&texture, None, dest_rect).unwrap();
        renderer.present();

        let elapsed = start_time.elapsed();
        log::info!("Recieved and drew frame {} in {} ms", frame_count, elapsed.as_millis());
        // delay to hit target FPS
        let target_latency = Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET);
        if elapsed < target_latency {
            std::thread::sleep(target_latency - elapsed);
        } else {
            log::warn!("Receiver took too long presenting; overshot frame deadline by {} ms", (elapsed - target_latency).as_millis());
        }
    }
}
