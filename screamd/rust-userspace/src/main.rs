use cpal::traits::{HostTrait, DeviceTrait, StreamTrait};
use cpal::{SampleRate, StreamConfig};
use libbpf_rs::{ObjectBuilder, OpenObject};
use std::sync::atomic::AtomicU64;

use nix::libc::{open, O_DIRECT, O_RDWR};

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}

static SLEEPT_AMT: AtomicU64 = AtomicU64::new(100);

fn main() {
    let host = cpal::default_host();
    let mic = host.input_devices().unwrap().find(|d| {
        d.name().map(|n| n.contains("sysdefault:CARD=Generic_1")).unwrap_or(false)
    }).expect("no microphone found");

    let supported_config = StreamConfig {
        buffer_size: cpal::BufferSize::Default,
        channels: 2,
        sample_rate: SampleRate(44100)
    }.into();
    eprintln!("Supported config: {:?}", supported_config);

    let stream = mic.build_input_stream(&supported_config, |data: &[f32], _: &cpal::InputCallbackInfo| {
        let mut sum = 0.0;
        for s in data {
            sum += (s * s).abs();
        }
        let rms = (sum / data.len() as f32).sqrt();
        let converted_rms = (rms * 1000.0) as u64;
        eprintln!("rms: {rms}, converted: {}, buf len: {1}", converted_rms as u64, data.len());
        (&SLEEPT_AMT).store(converted_rms, std::sync::atomic::Ordering::Relaxed);
    }, err_fn, None).unwrap();

    stream.play().unwrap();

    drop(stream);
}
