#![feature(generic_const_exprs)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rust_userspace::rtp::{RtpSizedPayloadReceiver, RtpSizedPayloadSender};
use std::net::UdpSocket;
use zerocopy::{FromBytes, IntoBytes, KnownLayout, Immutable};


#[derive(FromBytes, Debug, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
struct TestPayload {
    data: [u8; 64]
}

fn setup_sockets() -> (UdpSocket, UdpSocket) {
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    sender.connect(receiver.local_addr().unwrap()).unwrap();
    receiver.connect(sender.local_addr().unwrap()).unwrap();
    (sender, receiver)
}

fn bench_rtp_send_receive(c: &mut Criterion) {
    let mut group = c.benchmark_group("rtp");

    group.bench_function("send_receive", |b| {
        let (sender_socket, receiver_socket) = setup_sockets();
        let mut sender = RtpSizedPayloadSender::<TestPayload>::new(sender_socket);
        let receiver = RtpSizedPayloadReceiver::<TestPayload, 32>::new(receiver_socket);

        b.iter(|| {
            // Send a packet
            sender.send(|payload: &mut TestPayload| {
                payload.data = black_box([42u8; 64]);
            });

            // Wait for packet while releasing lock between checks
            loop {
                let has_packet = {
                    let receiver_lock = receiver.lock_receiver();
                    receiver_lock.peek_earliest_packet().is_some()
                };
                
                if has_packet {
                    break;
                }
                std::thread::yield_now();
            }

            // Now get and consume the packet
            let mut receiver_lock = receiver.lock_receiver();
            let packet = receiver_lock.consume_earliest_packet();
            black_box(packet.get_data().unwrap());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_rtp_send_receive);
criterion_main!(benches);
