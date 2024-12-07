use run_louder::*;

use bytes::{Buf, BufMut, Bytes};
use rtp::RtpSender;
use sdl2::{self, pixels::PixelFormatEnum};
use video::{decode_quantized_macroblock, dequantize_macroblock, encode_quantized_macroblock, quantize_macroblock, MacroblockWithPosition, MutableYUVFrame, YUVFrame, YUVFrameMacroblockIterator};
use std::{net::UdpSocket, time::Duration};

use simplelog::WriteLogger;

fn main() -> std::io::Result<()> {
    let log_file = std::io::BufWriter::with_capacity(
        65536 /* 64 KiB */,
        std::fs::File::create("send.log")?
    );

    WriteLogger::init(
        log::LevelFilter::Trace,
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