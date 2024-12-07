#![feature(generic_const_exprs)]

use rayon::iter::ParallelBridge;
use rayon::iter::ParallelIterator;
use run_louder::*;

use bytes::BufMut;
use rtp::RtpSender;
use video::{encode_quantized_macroblock, quantize_macroblock, MacroblockWithPosition, YUVFrame, YUVFrameMacroblockIterator};
use std::sync::Arc;
use std::sync::Mutex;
use std::{io::Write, time::Duration};

use simplelog::WriteLogger;

fn main() -> std::io::Result<()> {
    let log_file: Box<dyn Write + Send> = if BUFFER_LOGS {
        Box::new(std::io::BufWriter::with_capacity(
                    65536 /* 64 KiB */,
                    std::fs::File::create("send.log")?
                ))
    } else {
        Box::new(std::fs::File::create("send.log")?)
    };

    WriteLogger::init(
        LOG_LEVEL,
        simplelog::Config::default(),
        log_file,
    )
    .unwrap();

    // audio::send_audio();
    send_video();

    Ok(())
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

        fn accumulate_packets(frame: &YUVFrame<'_>, frame_count: u32, x: usize, y: usize, x_end: usize, y_end: usize, sender: Arc<Mutex<&mut RtpSender<[u8]>>>) {
            let mut packet_buf = Vec::with_capacity(PACKET_SEND_THRESHOLD);
            packet_buf.put_u32(frame_count);
            let mut current_macroblock_buf = Vec::with_capacity(PACKET_SEND_THRESHOLD);

            for MacroblockWithPosition {x, y, block} in YUVFrameMacroblockIterator::new_with_bounds(frame, x, y, x_end, y_end) {
                current_macroblock_buf.clear();

                let quantized_macroblock = quantize_macroblock(&block);

                current_macroblock_buf.put_u16(x as u16);
                current_macroblock_buf.put_u16(y as u16);
                encode_quantized_macroblock(&quantized_macroblock, &mut current_macroblock_buf);

                if packet_buf.len() + current_macroblock_buf.len() + 4 >= PACKET_SEND_THRESHOLD {
                    // send the packet and start a new one
                    packet_buf.put_u16(u16::MAX);
                    packet_buf.put_u16(u16::MAX);
                    sender.lock().unwrap().send(&packet_buf);
                    packet_buf.clear();
                    packet_buf.put_u32(frame_count);
                }

                // The macroblock consists of x, y, and the encoded macroblock
                log::trace!("Storing macroblock at ({}, {}, {}) at cursor position {}", frame_count, x, y, packet_buf.len());
                packet_buf.put_slice(&current_macroblock_buf);
            }

            // send leftover packet
            packet_buf.put_u16(u16::MAX);
            packet_buf.put_u16(u16::MAX);
            sender.lock().unwrap().send(&packet_buf);
        }

        let sender = Arc::new(Mutex::new(&mut sender));
        const PAR_PACKET_SPAN: usize = 32;
        assert!(PAR_PACKET_SPAN % PACKET_X_DIM == 0);
        assert!(PAR_PACKET_SPAN % PACKET_Y_DIM == 0);

        (0..VIDEO_WIDTH as u32).step_by(PAR_PACKET_SPAN).par_bridge().for_each(|x| {
            (0..VIDEO_HEIGHT as u32).step_by(PAR_PACKET_SPAN).for_each(|y| {
                accumulate_packets(&frame, frame_count, x as usize, y as usize, x as usize + PAR_PACKET_SPAN, y as usize + PAR_PACKET_SPAN, sender.clone());
            });
        });

        log::info!("Sent frame {}", frame_count);

        let elapsed = start_time.elapsed();
        // delay to hit target FPS
        let target_latency = Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET);
        if elapsed < target_latency {
            std::thread::sleep(target_latency - elapsed);
        } else {
            log::warn!("Sender took too long sending; overshot frame deadline by {} ms", (elapsed - target_latency).as_millis());
        }
        frame_count += 1;
    }
}