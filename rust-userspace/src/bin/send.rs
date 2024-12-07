#![feature(generic_const_exprs)]

use bytes::Buf;
use rayon::iter::ParallelBridge;
use rayon::iter::ParallelIterator;
use run_louder::*;

use bytes::BufMut;
use rtp::RtpSender;
use std::io::Read;
use std::net::TcpStream;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::{io::Write, time::Duration};
use video::{
    encode_quantized_macroblock, quantize_macroblock, MacroblockWithPosition, YUVFrame,
    YUVFrameMacroblockIterator,
};
use zerocopy::FromBytes;

use simplelog::WriteLogger;

fn main() -> std::io::Result<()> {
    let log_file: Box<dyn Write + Send> = if BUFFER_LOGS {
        Box::new(std::io::BufWriter::with_capacity(
            65536, /* 64 KiB */
            std::fs::File::create("send.log")?,
        ))
    } else {
        Box::new(std::fs::File::create("send.log")?)
    };

    WriteLogger::init(LOG_LEVEL, simplelog::Config::default(), log_file).unwrap();

    // audio::send_audio();
    send_video();

    Ok(())
}

fn receive_control(quality: Arc<RwLock<f64>>, mut stream: TcpStream) {
    let mut msg_buf = [0; size_of::<ControlMessage>()];
    log::info!("Listening for control server!");
    loop {
        stream.read_exact(&mut msg_buf).unwrap();
        let control_msg = ControlMessage::ref_from_bytes(&msg_buf).unwrap();
        log::debug!("Received quality update: {}", control_msg.quality);
        *quality.write().unwrap() = control_msg.quality;
    }
}

pub fn send_video() {
    let sock = std::net::UdpSocket::bind(VIDEO_SEND_ADDR).unwrap();

    sock.connect(VIDEO_DEST_ADDR).unwrap();

    log::info!(
        "Attempting to connect to control server at {}",
        CONTROL_SEND_ADDR
    );

    let quality = Arc::new(RwLock::new(0.3));

    // Connect timeout due to packet loss conditions
    let receiver_communication_socket = TcpStream::connect_timeout(
        &std::net::SocketAddr::from_str(CONTROL_RECV_ADDR).unwrap(),
        Duration::from_secs(3),
    )
    .unwrap();
    let cloned_quality = quality.clone();
    std::thread::spawn(|| {
        receive_control(cloned_quality, receiver_communication_socket);
    });

    let mut sender: RtpSender<[u8]> = rtp::RtpSender::new(sock);
    let sender = Arc::new(Mutex::new(&mut sender));

    log::info!("Starting to send video!");

    let mut camera = rscam::Camera::new("/dev/video1").unwrap();
    camera
        .start(&rscam::Config {
            interval: (1, 30),
            resolution: (VIDEO_WIDTH as _, VIDEO_HEIGHT as _),
            format: b"YUYV",
            ..Default::default()
        })
        .unwrap();

    let mut frame_count = 0;
    loop {
        let frame = camera.capture().unwrap();
        let frame: &[u8] = frame.as_ref();
        assert!(frame.len() % (VIDEO_WIDTH * PIXEL_WIDTH as u32) as usize == 0);
        assert!(frame.len() / (VIDEO_WIDTH as usize * PIXEL_WIDTH) == VIDEO_HEIGHT as usize);

        let start_time = std::time::Instant::now();

        let frame = YUVFrame::new(VIDEO_WIDTH as usize, VIDEO_HEIGHT as usize, frame);

        fn accumulate_packets(
            quality: Arc<RwLock<f64>>,
            frame: &YUVFrame<'_>,
            frame_count: u32,
            x: usize,
            y: usize,
            x_end: usize,
            y_end: usize,
            sender: Arc<Mutex<&mut RtpSender<[u8]>>>,
        ) {
            let mut packet_buf = Vec::with_capacity(PACKET_SEND_THRESHOLD);
            packet_buf.put_u32(frame_count);
            let mut current_macroblock_buf = Vec::with_capacity(PACKET_SEND_THRESHOLD);

            for MacroblockWithPosition { x, y, block } in
                YUVFrameMacroblockIterator::new_with_bounds(frame, x, y, x_end, y_end)
            {
                current_macroblock_buf.clear();

                // get quality
                // cycle quality between 0.3 and 0.03 based on the current time
                let quality = quality.read().unwrap().clone();

                let quantized_macroblock = quantize_macroblock(&block, quality);

                current_macroblock_buf.put_u16(x as u16);
                current_macroblock_buf.put_u16(y as u16);
                current_macroblock_buf.put_f64(quality);
                encode_quantized_macroblock(&quantized_macroblock, &mut current_macroblock_buf);

                if packet_buf.len() + current_macroblock_buf.len() + 4 >= PACKET_SEND_THRESHOLD {
                    // send the packet and start a new one
                    packet_buf.put_u16(u16::MAX);
                    packet_buf.put_u16(u16::MAX);
                    packet_buf.put_f64(0.0);
                    sender.lock().unwrap().send(&packet_buf);
                    packet_buf.clear();
                    packet_buf.put_u32(frame_count);
                }

                // The macroblock consists of x, y, and the encoded macroblock
                log::trace!(
                    "Storing macroblock at ({}, {}, {}) at cursor position {}",
                    frame_count,
                    x,
                    y,
                    packet_buf.len()
                );
                packet_buf.put_slice(&current_macroblock_buf);
            }

            // send leftover packet
            packet_buf.put_u16(u16::MAX);
            packet_buf.put_u16(u16::MAX);
            sender.lock().unwrap().send(&packet_buf);
        }

        const PAR_PACKET_SPAN: usize = 32;
        assert!(PAR_PACKET_SPAN % PACKET_X_DIM == 0);
        assert!(PAR_PACKET_SPAN % PACKET_Y_DIM == 0);

        (0..VIDEO_WIDTH as u32)
            .step_by(PAR_PACKET_SPAN)
            .par_bridge()
            .for_each(|x| {
                (0..VIDEO_HEIGHT as u32)
                    .step_by(PAR_PACKET_SPAN)
                    .for_each(|y| {
                        accumulate_packets(
                            quality.clone(),
                            &frame,
                            frame_count,
                            x as usize,
                            y as usize,
                            x as usize + PAR_PACKET_SPAN,
                            y as usize + PAR_PACKET_SPAN,
                            sender.clone(),
                        );
                    });
            });

        log::info!("Sent frame {}", frame_count);

        let elapsed = start_time.elapsed();
        // delay to hit target FPS
        let target_latency = Duration::from_secs_f64(1.0 / VIDEO_FPS_TARGET);
        if elapsed < target_latency {
            std::thread::sleep(target_latency - elapsed);
        } else {
            log::warn!(
                "Sender took too long sending; overshot frame deadline by {} ms",
                (elapsed - target_latency).as_millis()
            );
        }
        frame_count += 1;
    }
}
