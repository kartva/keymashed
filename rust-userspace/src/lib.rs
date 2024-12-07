#![feature(generic_const_exprs)]

pub mod bpf;
pub mod cli;
pub mod rtp;
pub mod audio;
pub mod video;

pub const VIDEO_WIDTH: u32 = 640;
pub const VIDEO_HEIGHT: u32 = 480;
pub const VIDEO_FPS_TARGET: f64 = 30.0;

pub const LOG_LEVEL: log::LevelFilter = log::LevelFilter::Warn;
pub const BUFFER_LOGS: bool = false;

pub const PACKET_SEND_THRESHOLD: usize = 1500;

pub const AUDIO_SEND_ADDR: &str = "127.0.0.1:44406";
pub const AUDIO_DEST_ADDR: &str = "127.0.0.1:44403";
pub const VIDEO_SEND_ADDR: &str = "127.0.0.1:44001";
pub const VIDEO_DEST_ADDR: &str = "127.0.0.1:44002";

pub const PIXEL_WIDTH: usize = 2;
pub const PACKET_X_DIM: usize = 16;
pub const PACKET_Y_DIM: usize = 16;
pub const PACKET_SIZE: usize = PACKET_X_DIM * PACKET_Y_DIM * PIXEL_WIDTH;