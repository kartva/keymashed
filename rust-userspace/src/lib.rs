#![feature(generic_const_exprs)]

use std::time::Duration;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub mod bpf;
pub mod wpm;
pub mod rtp;
pub mod audio;
pub mod video;

pub const VIDEO_WIDTH: u32 = 640;
pub const VIDEO_HEIGHT: u32 = 480;
pub const VIDEO_FPS_TARGET: f64 = 20.0;

pub const VIDEO_DELAY: Duration = Duration::from_secs(2 * 60);
// calculate frames per second, multiply by number of seconds to delay
pub const VIDEO_FRAME_DELAY: usize = (VIDEO_FPS_TARGET * VIDEO_DELAY.as_secs() as f64) as usize;

pub const LOG_LEVEL: log::LevelFilter = log::LevelFilter::Debug;
pub const BUFFER_LOGS: bool = true;

pub const PACKET_SEND_THRESHOLD: usize = 1500;

pub const AUDIO_SEND_ADDR: &str = "127.0.0.1:44406";
pub const AUDIO_DEST_ADDR: &str = "127.0.0.1:44403";

pub const RECV_HACKERS_IP: &str = "100.100.1.141";


lazy_static::lazy_static! {
    /// The video receiver sends control information to this address.
    pub static ref CONTROL_SEND_ADDR: &'static str = std::env::var("CONTROL_SEND_ADDR").unwrap_or_else(|_| "100.100.1.174:44601".to_string()).leak();
    /// The video sender receives control information from this address.
    /// The video receiver hosts the control server on this address.
    pub static ref CONTROL_RECV_ADDR: &'static str = std::env::var("CONTROL_RECV_ADDR").unwrap_or_else(|_| "100.100.1.141:51902".to_string()).leak();
    /// The video sender sends video data to this address.
    pub static ref VIDEO_SEND_ADDR: &'static str = std::env::var("VIDEO_SEND_ADDR").unwrap_or_else(|_| "100.100.1.174:44001".to_string()).leak();
    /// The video receiver receives video data from this address.
    pub static ref VIDEO_DEST_ADDR: &'static str = std::env::var("VIDEO_DEST_ADDR").unwrap_or_else(|_| "100.100.1.141:44002".to_string()).leak();
}

pub const PIXEL_WIDTH: usize = 2;
pub const PACKET_X_DIM: usize = 16;
pub const PACKET_Y_DIM: usize = 16;
pub const PACKET_SIZE: usize = PACKET_X_DIM * PACKET_Y_DIM * PIXEL_WIDTH;

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone, Copy)]
pub struct ControlMessage {
    pub quality: f64,
}

pub fn udp_connect_retry(addr: &str) -> std::net::UdpSocket {
    loop {
        if let Ok(s) = std::net::UdpSocket::bind(*VIDEO_SEND_ADDR) {
            break s
        } else {
            log::error!("Failed to bind to {addr}; retrying in 2 second");
            eprintln!("Failed to bind to {addr}; retrying in 2 seconds");
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}
