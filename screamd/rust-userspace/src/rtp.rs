use rand::seq::SliceRandom;
use rand::thread_rng;
use std::{
    collections::VecDeque, mem::{ManuallyDrop, MaybeUninit}, net::UdpSocket, sync::{
        mpsc::{channel, RecvTimeoutError}, Arc, MappedMutexGuard, Mutex, MutexGuard
    }, thread::sleep, time::{Duration, Instant}
};

use bytes::{buf, Buf, BufMut, Bytes, BytesMut};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, TryFromBytes};
use zerocopy::byteorder::network_endian::U32;
use std::fmt::Debug;

#[derive(Debug, TryFromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct Packet<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> {
    seq: U32,
    data: T,
}

union PacketUnion<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> where 
    [u8; std::mem::size_of::<Packet<T>>()]: Sized
{
    packet: ManuallyDrop<Packet<T>>,
    raw: [u8; std::mem::size_of::<Packet<T>>()],
}

struct MaybeInitPacket<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> where 
    [u8; std::mem::size_of::<Packet<T>>()]: Sized
{
    init: bool,
    packet: PacketUnion<T>,
}

#[repr(C)]
struct RtpStateInner<T: Sized + TryFromBytes + IntoBytes + KnownLayout + Immutable> where [(); std::mem::size_of::<Packet<T>>()]: Sized
{
    earliest_seq: u32,
    buf: Box<[MaybeInitPacket<T>; 1024]>,
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> RtpStateInner<T> where [(); std::mem::size_of::<Packet<T>>()]: Sized {
    pub const fn generate_default_packet() -> MaybeInitPacket<T> {
        MaybeInitPacket {init: false, packet: PacketUnion {raw: [0u8; std::mem::size_of::<Packet<T>>()]} }
    }

    fn new() -> Self {
        RtpStateInner {
            earliest_seq: 0,
            buf: Box::new([const { Self::generate_default_packet() }; 1024]),
        }
    }
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> Drop for RtpStateInner<T> where [(); std::mem::size_of::<Packet<T>>()]: Sized {
    fn drop(&mut self) {
        for MaybeInitPacket {init, packet} in self.buf.iter_mut() {
            if *init {
                unsafe {
                    ManuallyDrop::drop(&mut packet.packet);
                }
            }
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

        self.sock.send(&packet).unwrap();
        log::debug!("Sent packet with seq: {}", self.seq_num);
        self.seq_num = self.seq_num.wrapping_add(1);
        self.scratch.clear();
    }
}

pub struct RtpReciever<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> where [(); std::mem::size_of::<Packet<T>>()]: Sized {
    inner: Arc<Mutex<RtpStateInner<T>>>,
}

struct ReceivedPacket<'a, T: TryFromBytes + IntoBytes + KnownLayout + Immutable> (MutexGuard<'a, RtpStateInner<T>>) where [(); std::mem::size_of::<Packet<T>>()]: Sized;
impl <'a, T: TryFromBytes + IntoBytes + KnownLayout + Immutable> ReceivedPacket<'a, T> where [(); std::mem::size_of::<Packet<T>>()]: Sized {
    fn get_data(&self) -> &Packet<T> {
        unsafe {&self.0.buf[self.0.earliest_seq as usize].packet.packet}
    }
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> Drop for ReceivedPacket<'_, T> where [(); std::mem::size_of::<Packet<T>>()]: Sized {
    fn drop(&mut self) {
        let idx = self.0.earliest_seq as usize;
        self.0.buf[idx].init = false;
        self.0.earliest_seq = self.0.earliest_seq.wrapping_add(1);
    }
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable + Send + 'static + Debug> RtpReciever<T> where [(); std::mem::size_of::<Packet<T>>()]: Sized {
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

    fn ask_for_packet<'a>(&'a self) -> Option<ReceivedPacket<'a, T>> {
        let mut state = self.inner.lock().unwrap();
        let earliest_seq = state.earliest_seq;
        let packets = &mut state.buf;

        if let Some(MaybeInitPacket {init: true, packet: _ }) = packets.get(earliest_seq as usize) {
            // receivedpacket destructor will update earliest_seq
            return Some(ReceivedPacket(state));
        }
        state.earliest_seq = state.earliest_seq.wrapping_add(1);
        None
    }
}

fn accept_thread<T: TryFromBytes + IntoBytes + KnownLayout + Immutable + Debug> (sock: UdpSocket, recv: Arc<Mutex<RtpStateInner<T>>>) where [(); std::mem::size_of::<Packet<T>>()]: Sized {
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
        let idx = seq_num.wrapping_sub(earliest_seq) as usize;

        if idx >= packets.len() {
            log::info!(
                "Dropping packet with seq: {} for being too early/late; {idx} >= {}",
                seq_num, packets.len()
            );
            continue;
        }

        let MaybeInitPacket {init, packet} = &mut state.buf[idx];

        // Prepare a raw buffer with the known layout size of Packet<T>
        let mut raw_buffer = unsafe {
            packet.raw
        };

        sock.recv(&mut raw_buffer).unwrap();

        log::debug!("Received packet with raw data: {:?}", &raw_buffer);

        if let Ok(_) = Packet::<T>::try_ref_from_bytes(&raw_buffer) {
            *init = true;
            log::info!("Received packet with seq: {} and content {:?}", unsafe {packet.packet.seq}, unsafe {&packet.packet.data});
        } else {
            log::info!("Failed to parse packet with seq: {}", seq_num);
        }
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
        if let Some(packet) = recv.ask_for_packet() {
            log::info!("Playing packet with seq: {}", packet.get_data().seq);
            // play audio
            // write to audio api

        }

        // sleep for a few milliseconds (2048 / 44100 - a lil bit)
        // to allow more packets to arrive
        std::thread::sleep(Duration::from_millis(45));
    }
}
