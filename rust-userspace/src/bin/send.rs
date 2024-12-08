#![feature(generic_const_exprs)]

use memmap::MmapMut;
use rayon::iter::ParallelBridge;
use rayon::iter::ParallelIterator;
use run_louder::*;

use bytes::BufMut;
use rtp::RtpSender;
use std::convert::Infallible;
use std::net::Ipv4Addr;
use std::net::UdpSocket;
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
    run_louder::init_logger(true);
    send_video();

    Ok(())
}

fn receive_control(quality: Arc<RwLock<f64>>, stream: UdpSocket) {
    let mut msg_buf = [0; size_of::<ControlMessage>()];
    log::info!("Listening for control server!");
    loop {
        stream.recv(&mut msg_buf).unwrap();
        let control_msg = ControlMessage::ref_from_bytes(&msg_buf).unwrap();
        log::debug!("Received quality update: {}", control_msg.quality);
        *quality.write().unwrap() = control_msg.quality;
    }
}

struct DummyWebcam {
    frame_count: u32,
    frame: Vec<u8>,
}

impl DummyWebcam {
    fn new(height: usize, width: usize) -> Self {
        Self {
            frame_count: 0,
            frame: Vec::with_capacity(height * width * 2),
        }
    }

    fn capture(&mut self) -> Result<&[u8], Infallible> {
        self.frame_count += 1;
        self.frame.clear();
        self.frame
            .resize(VIDEO_WIDTH as usize * VIDEO_HEIGHT as usize * 2, 0);
        let frame = self.frame.as_mut_slice();

        for y in 0..VIDEO_HEIGHT as usize {
            for x in 0..VIDEO_WIDTH as usize {
                let pixel = &mut frame[(y * VIDEO_WIDTH as usize + x) * PIXEL_WIDTH as usize..(y * VIDEO_WIDTH as usize + x) * PIXEL_WIDTH as usize + 2];
                pixel[0] = ((x + (self.frame_count as usize)) % u8::MAX as usize) as u8;
                pixel[1] = ((y + (2 * self.frame_count as usize)) % u8::MAX as usize) as u8;
            }
        }

        Ok(self.frame.as_slice())
    }
}


const FRAME_CIRCULAR_BUFFER_SIZE: usize =
    VIDEO_FRAME_DELAY * VIDEO_HEIGHT as usize * VIDEO_WIDTH as usize * 2;

struct FrameCircularBuffer {
    buffer: Box<[u8; FRAME_CIRCULAR_BUFFER_SIZE]>,
    start_frame_num: usize,
    end_frame_num: usize,
}

impl FrameCircularBuffer {
    pub fn new() -> Self {
        Self {
            buffer: Box::new([0; FRAME_CIRCULAR_BUFFER_SIZE]),
            start_frame_num: 0,
            end_frame_num: 0,
        }
    }

    pub fn push_frame(&mut self, frame: &[u8]) {
        if self.start_frame_num == (self.end_frame_num + 1) % VIDEO_FRAME_DELAY {
            log::error!("Frame buffer full; dropping frame");
            return;
        }

        // copy frame into buffer
        self.buffer[(self.end_frame_num * VIDEO_HEIGHT as usize * VIDEO_WIDTH as usize * 2)
            ..((self.end_frame_num + 1) * VIDEO_HEIGHT as usize * VIDEO_WIDTH as usize * 2)]
            .copy_from_slice(frame);
        self.end_frame_num = (self.end_frame_num + 1) % VIDEO_FRAME_DELAY;
    }

    pub fn pop_frame(&mut self) -> Option<&[u8]> {
        if self.start_frame_num != self.end_frame_num {
            let frame = &self.buffer[(self.start_frame_num
                * VIDEO_HEIGHT as usize
                * VIDEO_WIDTH as usize
                * 2)
                ..((self.start_frame_num + 1) * VIDEO_HEIGHT as usize * VIDEO_WIDTH as usize * 2)];
            self.start_frame_num = (self.start_frame_num + 1) % VIDEO_FRAME_DELAY;
            Some(frame)
        } else {
            None
        }
    }
}

pub fn send_video() {
    log::info!("Starting camera!");

    let mut camera = rscam::Camera::new("/dev/video0").unwrap();

    dbg!(camera.intervals(b"YUYV", (VIDEO_WIDTH as _, VIDEO_HEIGHT as _)).expect("interval information is available"));

    camera
        .start(&rscam::Config {
            interval: (1, VIDEO_FPS_TARGET as _),
            resolution: (VIDEO_WIDTH as _, VIDEO_HEIGHT as _),
            format: b"YUYV",
            ..Default::default()
        })
        .unwrap();

    let sock = udp_connect_retry((Ipv4Addr::UNSPECIFIED, SEND_VIDEO_PORT));
    sock.connect((RECV_HACKERS_IP, RECV_VIDEO_PORT)).unwrap();

    let receiver_communication_socket = udp_connect_retry((Ipv4Addr::UNSPECIFIED, SEND_CONTROL_PORT));
    receiver_communication_socket.connect((RECV_HACKERS_IP, RECV_CONTROL_PORT)).unwrap();
    
    let quality = Arc::new(RwLock::new(0.3));
    let cloned_quality = quality.clone();
    std::thread::spawn(|| {
        receive_control(cloned_quality, receiver_communication_socket);
    });

    let mut sender: RtpSender<[u8]> = rtp::RtpSender::new(sock);
    let sender = Arc::new(Mutex::new(&mut sender));

    let mut frame_delay_buffer = FrameCircularBuffer::new();
    let mut frame_count = 0;

    let mut dummy_camera = DummyWebcam::new(VIDEO_HEIGHT as usize, VIDEO_WIDTH as usize);
    for _ in 0..VIDEO_FRAME_DELAY {
        let frame = dummy_camera.capture().unwrap();
        frame_delay_buffer.push_frame(frame.as_ref());
    }
    drop(dummy_camera);

    loop {
        let frame = camera.capture().unwrap();
        let frame: &[u8] = frame.as_ref();
        frame_delay_buffer.push_frame(frame);

        let frame = match frame_delay_buffer.pop_frame() {
            Some(frame) => frame,
            None => panic!("Frame buffer empty"),
        };

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

        const PAR_PACKET_SPAN: usize = 16;
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
