#![feature(generic_const_exprs)]
#![feature(mapped_lock_guards)]

mod bpf;
mod cli;
mod multimedia;
mod rtp;

use simplelog::WriteLogger;
use log;

fn main () -> std::io::Result<()> {
    // let (sender, receiver) = sync_channel(10);

    let log_file = std::fs::File::create("rust-userspace.log")?;

    WriteLogger::init(log::LevelFilter::Trace, simplelog::Config::default(), log_file).unwrap();

    // std::thread::spawn(move || {
    //     log::info!("Starting BPF thread");
    //     let bpf_handle = unsafe { bpf::init().unwrap() };
    //     log::info!("BPF map found and opened");
    //     loop {
    //         match receiver.recv() {
    //             Ok(val) => bpf_handle.write_to_map(0, val).unwrap(),
    //             Err(_) => break,
    //         }
    //     }
    // });
    // cli::main(sender)

    std::thread::spawn(|| {
        rtp::send_audio();
    });

    rtp::play_audio();

    Ok(())
}