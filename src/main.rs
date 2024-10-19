use cpal::traits::{HostTrait, DeviceTrait, StreamTrait};
use cpal::{SampleRate, StreamConfig};
use nix::libc::{sleep, usleep, waitpid};
use std::process::Command;
use std::process::exit;
use std::sync::atomic::AtomicU64;

use nix::sys::ptrace;
use nix::sys::wait::{WaitStatus, wait};
use nix::unistd::{fork, ForkResult, Pid};

use std::os::unix::process::CommandExt;

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}

static SLEEPT_AMT: AtomicU64 = AtomicU64::new(100);

fn main() {
    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            ptrace::traceme().unwrap();
            Command::new("firefox").exec();
            exit(0);
        }
        Ok(ForkResult::Parent { child }) => {
            let host = cpal::default_host();
            let mic = host.input_devices().unwrap().find(|d| {
                d.name().map(|n| n.contains("sysdefault:CARD=Generic_1")).unwrap_or(false)
            }).expect("no microphone found");
        
            // let mic = host.default_input_device().expect("no default input device");
        
            let supported_config = StreamConfig {
                buffer_size: cpal::BufferSize::Default,
                channels: 2,
                sample_rate: SampleRate(44100)
            };
            eprintln!("Supported config: {:?}", supported_config);
        
            let stream = mic.build_input_stream(&supported_config.into(), |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut sum = 0.0;
                for s in data {
                    sum += (s * s).abs();
                }
                let rms = (sum / data.len() as f32).sqrt();
                let converted_rms = (rms * 1000.0 / 2.0) as u64;
                eprintln!("rms: {}", converted_rms as u64);
                (&SLEEPT_AMT).store(converted_rms, std::sync::atomic::Ordering::Relaxed);
            }, err_fn, None).unwrap();
        
            stream.play().unwrap();
        
            let mut ctr = 0;
            loop {
                // we ignore this error because the Zig code I copied did,
                // and it works after ignoring the error
                let _ = ptrace::syscall(child, None);
                
                if nix::sys::wait::waitpid(child, None).unwrap() == WaitStatus::Exited(child, 0) {
                    break;
                }

                let regs = ptrace::getregs(child).unwrap();
                let fun = regs.orig_rax;

                if fun == nix::libc::SYS_poll as u64 {
                    // eprintln!("poll syscall {ctr}");
                    ctr += 1;
                    let sleep_amt = (&SLEEPT_AMT).load(std::sync::atomic::Ordering::Relaxed) as u32;
                    let decided_amt = 1000u32.checked_sub(3 * sleep_amt);
                    if let Some(sleep_amt) = decided_amt {
                        unsafe {
                            usleep(sleep_amt)
                        };
                    }
                }
            }

            drop(stream);
        },
        Err(_) => {
            eprintln!("fork failed");
        }
    }
}
