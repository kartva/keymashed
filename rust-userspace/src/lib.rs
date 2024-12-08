#![feature(generic_const_exprs)]

use std::{net::UdpSocket, time::Duration};

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub mod audio;
pub mod bpf;
pub mod rtp;
pub mod video;
pub mod wpm;

pub const VIDEO_WIDTH: u32 = 640;
pub const VIDEO_HEIGHT: u32 = 480;
pub const VIDEO_FPS_TARGET: f64 = 20.0;

pub const VIDEO_DELAY: Duration = Duration::from_secs(30);
// calculate frames per second, multiply by number of seconds to delay
pub const VIDEO_FRAME_DELAY: usize = (VIDEO_FPS_TARGET * VIDEO_DELAY.as_secs() as f64) as usize;

pub const LOG_LEVEL: log::LevelFilter = log::LevelFilter::Warn;
// pub const BUFFER_LOGS: bool = true;

pub const PACKET_SEND_THRESHOLD: usize = 1500;

pub const AUDIO_SEND_ADDR: &str = "127.0.0.1:44406";
pub const AUDIO_DEST_ADDR: &str = "127.0.0.1:44403";

pub const RECV_HACKERS_IP: &str = "100.100.1.141";
pub const SENDER_FFFF_IP: &str = "100.100.1.174";

pub const RECV_VIDEO_PORT: u16 = 44002;
pub const SEND_VIDEO_PORT: u16 = 44001;
pub const RECV_CONTROL_PORT: u16 = 51902;
pub const SEND_CONTROL_PORT: u16 = 44601;

pub const PIXEL_WIDTH: usize = 2;
pub const PACKET_X_DIM: usize = 16;
pub const PACKET_Y_DIM: usize = 16;
pub const PACKET_SIZE: usize = PACKET_X_DIM * PACKET_Y_DIM * PIXEL_WIDTH;

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone, Copy)]
pub struct ControlMessage {
    pub quality: f64,
}

pub fn init_logger(_is_send: bool) {
    simplelog::SimpleLogger::init(LOG_LEVEL, simplelog::Config::default()).unwrap();
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
