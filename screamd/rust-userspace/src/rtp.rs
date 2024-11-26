use rand::seq::SliceRandom;
use rand::thread_rng;
use std::{
    collections::VecDeque,
    sync::{
        mpsc::{channel, RecvTimeoutError},
        Arc, Mutex,
    },
    thread::sleep,
    time::{Duration, Instant},
};

use bytes::{buf, Buf, BufMut, Bytes, BytesMut};

struct RtpStateInner {
    earliest_seq: u32,
    buf: Box<[Option<AudioPacket>; 1024]>,
    has_packets: std::sync::Condvar,
}

pub struct RtpState {
    inner: Arc<Mutex<RtpStateInner>>,
}

impl RtpState {
    fn new() -> Self {
        RtpState {
            inner: Arc::new(Mutex::new(RtpStateInner {
                earliest_seq: 0,
                buf: Box::new([const { None }; 1024]),
                has_packets: std::sync::Condvar::new(),
            })),
        }
    }
}

fn to_u8_slice_mut(slice: &mut [u16]) -> &mut [u8] {
    let byte_len = 2 * slice.len();
    unsafe { std::slice::from_raw_parts_mut(slice.as_mut_ptr().cast::<u8>(), byte_len) }
}

fn to_u8_slice(slice: &[u16]) -> &[u8] {
    let byte_len = 2 * slice.len();
    unsafe { std::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), byte_len) }
}

#[derive(Debug, Clone)]
pub struct AudioPacket {
    seq: u32,
    data: [u16; 2048],
}

pub fn send_reordered_packets() {
    let sock = std::net::UdpSocket::bind("127.0.0.1:44001").unwrap();
    sock.connect("127.0.0.1:44002").unwrap();
    let mut seq_num = 0;
    let bytes = vec![1u16; 2048];
    // give the receiver some time to start
    sleep(Duration::from_secs(1));

    loop {
        let mut packets = Vec::new();
        for _ in 0..10 {
            let mut packet = BytesMut::new();
            packet.put_u32(seq_num);
            packet.put(to_u8_slice(bytes.as_slice()));
            packets.push(packet);
            seq_num += 1;
        }

        // Shuffle the packets to reorder them
        let mut rng = thread_rng();
        packets.shuffle(&mut rng);

        for packet in packets {
            sock.send(&packet).unwrap();
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}

pub fn send_thread() {
    let sock = std::net::UdpSocket::bind("127.0.0.1:44001").unwrap();
    sock.connect("127.0.0.1:44002").unwrap();
    let mut seq_num = 0;
    let bytes = vec![1u16; 2048];
    // give the receiver some time to start
    sleep(Duration::from_secs(1));
    loop {
        let mut packet = BytesMut::new();
        packet.put_u32(seq_num);
        packet.put(to_u8_slice(bytes.as_slice()));

        sock.send(&packet).unwrap();
        seq_num += 1;
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn accept_thread(sender: std::sync::mpsc::Sender<AudioPacket>) {
    let sock = std::net::UdpSocket::bind("127.0.0.1:44002").unwrap();
    log::info!("Receiver started listening on 127.0.0.1:44002.");
    let mut bytes = vec![0u16; 65600];
    // upto 1024 packets are allowed in the queue
    // 44100 samples per second
    // 2048 samples per packet
    // 44100 / 2048 = 21.53 audio packets per second
    // 2048 / 44100 = ~0.0463 seconds of audio per packet
    // 1024 packets => 1024 / 21.53 = 47.6 seconds of audio

    loop {
        let _len = sock.recv(to_u8_slice_mut(bytes.as_mut_slice())).unwrap();
        let seq_num = to_u8_slice(bytes.as_slice()).get_u32();
        log::info!("Received packet with seq: {}", seq_num);
        // assert_eq!(len * 8, 4 + (2048 * 16));
        let mut samples = [0u16; 2048];
        samples.copy_from_slice(&bytes[2..(2 + 2048)]);

        sender
            .send(AudioPacket {
                seq: seq_num,
                data: samples,
            })
            .unwrap();
    }
}

fn buffer(state: RtpState) {
    let (sender, receiver) = channel();
    std::thread::spawn(move || {
        accept_thread(sender);
    });

    loop {
        match receiver.recv() {
            Ok(packet) => {
                let mut state = state.inner.lock().unwrap();
                let earliest_seq = state.earliest_seq;
                let packets = &mut state.buf;

                let idx = packet.seq.wrapping_sub(earliest_seq) as usize;

                if idx >= packets.len() {
                    log::info!(
                        "Dropping packet with seq: {} for being too early/late; {idx} >= {}",
                        packet.seq, packets.len()
                    );
                    continue;
                }

                packets[idx] = Some(packet);
            }
            Err(_) => {
                break;
            }
        }
    }
}

pub fn play_audio() {
    let state = RtpState::new();
    let cloned_state = RtpState {
        inner: state.inner.clone(),
    };

    std::thread::spawn(move || {
        buffer(cloned_state);
    });

    std::thread::sleep(Duration::from_secs(1));

    loop {
        let mut packets = state.inner.lock().unwrap();

        if let Some(Some(packet)) = packets.buf.get(packets.earliest_seq as usize) {
            log::info!("Playing packet with seq: {}", packet.seq);
            // play audio
            // write to audio api

        }
        // update earliest_seq
        packets.earliest_seq = packets.earliest_seq.wrapping_add(1);
        drop(packets);

        // sleep for a few milliseconds (2048 / 44100 - a lil bit)
        // to allow more packets to arrive
        std::thread::sleep(Duration::from_millis(45));
    }
}
