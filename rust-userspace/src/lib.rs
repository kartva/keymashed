#![feature(generic_const_exprs)]

use std::{io::Write, net::UdpSocket, time::Duration};

use simplelog::WriteLogger;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub mod audio;
pub mod bpf;
pub mod rtp;
pub mod video;
pub mod wpm;

pub const VIDEO_WIDTH: u32 = 640;
pub const VIDEO_HEIGHT: u32 = 480;
pub const VIDEO_FPS_TARGET: f64 = 30.0;

pub const VIDEO_DELAY: Duration = Duration::from_secs(1);
// calculate frames per second, multiply by number of seconds to delay
pub const VIDEO_FRAME_DELAY: usize = (VIDEO_FPS_TARGET * VIDEO_DELAY.as_secs() as f64) as usize;

pub const LOG_LEVEL: log::LevelFilter = log::LevelFilter::Warn;
pub const BUFFER_LOGS: bool = false;

/// Maximum size of packet payloads. (Tries to correspond to Ethernet MTU)
pub const PACKET_PAYLOAD_SIZE_THRESHOLD: usize = 1400;

/// IP address of the machine running the `recv` binary.
pub const RECV_IP: &str = "127.0.0.1";
/// IP address of the machine running the `send` binary.
pub const SEND_IP: &str = "127.0.0.1";

/// Port on recv for audio data.
pub const RECV_AUDIO_PORT: u16 = 44403;
/// Port on send for audio data.
pub const SEND_AUDIO_PORT: u16 = 44406;
/// Port on recv for video data.
pub const RECV_VIDEO_PORT: u16 = 44002;
/// Port on send for video data.
pub const SEND_VIDEO_PORT: u16 = 44001;
/// Port on recv for control messages.
pub const RECV_CONTROL_PORT: u16 = 51902;
/// Port on send for control messages.
pub const SEND_CONTROL_PORT: u16 = 44601;

pub const PIXEL_WIDTH: usize = 2;
pub const MACROBLOCK_X_DIM: usize = 16;
pub const MACROBLOCK_Y_DIM: usize = 16;
pub const MACROBLOCK_BYTE_SIZE: usize = MACROBLOCK_X_DIM * MACROBLOCK_Y_DIM * PIXEL_WIDTH;

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone, Copy)]
pub struct ControlMessage {
    pub quality: f64,
}

pub fn init_logger(_is_send: bool) {
    let log_file_name = if _is_send { "send.log" } else { "recv.log" };

    let log_file: Box<dyn Write + Send> = if BUFFER_LOGS {
        Box::new(std::io::BufWriter::with_capacity(
            65536 /* 64 KiB */,
            std::fs::File::create(log_file_name).unwrap()
        ))
    } else {
        Box::new(std::fs::File::create(log_file_name).unwrap())
    };
    WriteLogger::init(
        LOG_LEVEL,
        simplelog::Config::default(),
        log_file,
    )
    .unwrap();
}

pub fn udp_send_retry(sock: &UdpSocket, buf: &[u8]) {
    while let Err(e) = sock.send(buf) {
        log::error!("Error sending packet from {:?} -> {:?}: {}", sock.peer_addr(), sock.local_addr(), e);
        std::thread::sleep(Duration::from_millis(500));
    }
}

pub fn udp_connect_retry<A>(addr: A) -> std::net::UdpSocket
where
    A: std::net::ToSocketAddrs + std::fmt::Debug,
{
    loop {
        if let Ok(s) = std::net::UdpSocket::bind(&addr) {
            break s;
        } else {
            log::error!("Failed to bind to {addr:?}; retrying in 2 second");
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}
