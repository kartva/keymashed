use std::{
    net::UdpSocket, sync::{
        Arc, Mutex, MutexGuard
    }, time::Duration
};

use bytes::{BufMut, BytesMut};
use zerocopy::{Immutable, IntoBytes, KnownLayout, TryFromBytes};
use zerocopy::byteorder::network_endian::U32;
use std::fmt::Debug;

#[derive(Debug, TryFromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct Packet<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> {
    seq: U32,
    data: T,
}

const fn size_of_packet<T: TryFromBytes + IntoBytes + KnownLayout + Immutable>() -> usize {
    std::mem::size_of::<Packet<T>>()
}

struct MaybeInitPacket<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> where 
    [u8; size_of_packet::<T>()]: Sized
{
    init: bool,
    packet: [u8; size_of_packet::<T>()],
}

#[repr(C)]
struct RtpStateInner<T: Sized + TryFromBytes + IntoBytes + KnownLayout + Immutable> where [(); size_of_packet::<T>()]: Sized
{
    earliest_seq: u32,
    buf: Box<[MaybeInitPacket<T>; 1024]>,
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> RtpStateInner<T> where [(); size_of_packet::<T>()]: Sized {
    pub const fn generate_default_packet() -> MaybeInitPacket<T> {
        MaybeInitPacket {init: false, packet: [0u8; size_of_packet::<T>()] }
    }

    fn new() -> Self {
        RtpStateInner {
            earliest_seq: 0,
            buf: Box::new([const { Self::generate_default_packet() }; 1024]),
        }
    }
}

pub struct RtpSender {
    sock: UdpSocket,
    seq_num: u32,
    scratch: BytesMut,
}

impl RtpSender {
    /// Create a new RTP send state. 
    fn new(sock: UdpSocket) -> Self {
        sock.set_nonblocking(true).unwrap();
        RtpSender {
            sock,
            seq_num: 0,
            scratch: BytesMut::with_capacity(2048),
        }
    }

    /// Send a packet over the network.
    /// This _may_ block, depending on Linux's network stack.

    fn send<T: IntoBytes + Immutable + ?Sized, A: AsRef<T>>(&mut self, data: A) {
        let packet = &mut self.scratch;
        packet.put_u32(self.seq_num);
        packet.put(data.as_ref().as_bytes());

        self.sock.send(packet).unwrap();
        log::debug!("Sent packet with seq: {}", self.seq_num);
        self.seq_num = self.seq_num.wrapping_add(1);
        self.scratch.clear();
    }
}

pub struct RtpReciever<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> where [(); size_of_packet::<T>()]: Sized {
    inner: Arc<Mutex<RtpStateInner<T>>>,
}

struct ReceivedPacket<'a, T: TryFromBytes + IntoBytes + KnownLayout + Immutable> (MutexGuard<'a, RtpStateInner<T>>) where [(); size_of_packet::<T>()]: Sized;
impl <T: TryFromBytes + IntoBytes + KnownLayout + Immutable> ReceivedPacket<'_, T> where [(); size_of_packet::<T>()]: Sized {
    fn get_data(&self) -> Option<&Packet<T>> {
        if let Some(MaybeInitPacket {init: true, packet: _ }) = self.0.buf.get((self.0.earliest_seq as usize) % self.0.buf.len()) {
            Some(Packet::<T>::try_ref_from_bytes(&self.0.buf[self.0.earliest_seq as usize].packet).unwrap())
        } else {
            None
        }
    }
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> Drop for ReceivedPacket<'_, T> where [(); size_of_packet::<T>()]: Sized {
    fn drop(&mut self) {
        let idx = self.0.earliest_seq as usize;
        self.0.buf[idx].init = false;
        self.0.earliest_seq = self.0.earliest_seq.wrapping_add(1);
    }
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable + Send + 'static + Debug> RtpReciever<T> where [(); size_of_packet::<T>()]: Sized {
    /// Launches listener thread that recieves packets and stores them in a buffer.

    fn new(sock: UdpSocket) -> Self {
        let inner = Arc::new(Mutex::new(RtpStateInner::new()));

        let cloned_recv = inner.clone();
        std::thread::spawn(move || {
            accept_thread(sock, cloned_recv);
        });
        
        RtpReciever {
            inner,
        }
    }

    fn ask_for_packet(&self) -> ReceivedPacket<'_, T> {
        let state = self.inner.lock().unwrap();
        ReceivedPacket(state)
    }
}

fn accept_thread<T: TryFromBytes + IntoBytes + KnownLayout + Immutable + Debug> (sock: UdpSocket, recv: Arc<Mutex<RtpStateInner<T>>>) where [(); size_of_packet::<T>()]: Sized {
    sock.set_nonblocking(false).unwrap();
    log::info!("Receiver started listening on {:?}.", sock.local_addr());
    // upto 1024 packets are allowed in the queue
    // 44100 samples per second
    // 2048 samples per packet
    // 44100 / 2048 = 21.53 audio packets per second
    // 2048 / 44100 = ~0.0463 seconds of audio per packet
    // 1024 packets => 1024 / 21.53 = 47.6 seconds of audio

    loop {
        // wait until socket has a packet to read
        let mut seq_num = [0u8; 4];
        sock.peek(&mut seq_num).unwrap();

        // we have available data to read
        let mut state = recv.lock().unwrap();
        let earliest_seq = state.earliest_seq;
        let packets = &mut state.buf;

        let seq_num: u32 = U32::from_bytes(seq_num).into();

        if seq_num.wrapping_sub(earliest_seq) as usize >= packets.len() {
            log::info!(
                "Dropping packet with seq: {} for being too early/late; {seq_num} >= {}",
                seq_num, packets.len()
            );
            continue;
        }

        let idx = (seq_num as usize) % packets.len();
        let MaybeInitPacket {init, packet} = &mut state.buf[idx];

        // Prepare a raw buffer with the known layout size of Packet<T>
        sock.recv(packet).unwrap();

        log::debug!("Received packet with raw data: {:?}", packet);
        *init = true;
    }
}

pub fn send_audio() {
    let sock = std::net::UdpSocket::bind("127.0.0.1:44001").unwrap();
    sock.connect("127.0.0.1:44002").unwrap();
    let mut sender = RtpSender::new(sock);

    let bytes = [5u8; 4];

    loop {
        sender.send(bytes);
        std::thread::sleep(Duration::from_millis(45));
    }
}

pub fn play_audio() {
    let sock = std::net::UdpSocket::bind("127.0.0.1:44002").unwrap();
    let recv: RtpReciever<[u8; 4]> = RtpReciever::new(sock);

    std::thread::sleep(Duration::from_secs(1));

    loop {
        if let Some(packet) = recv.ask_for_packet().get_data() {
            log::info!("Playing packet with seq: {}", packet.seq);
            // play audio
            // write to audio api

        }

        // sleep for a few milliseconds (2048 / 44100 - a lil bit)
        // to allow more packets to arrive
        std::thread::sleep(Duration::from_millis(45));
    }
}
