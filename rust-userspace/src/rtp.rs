use std::{
    fmt::Debug,
    net::UdpSocket,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use bytes::{BufMut, BytesMut};
use zerocopy::byteorder::network_endian::U32;
use zerocopy::{Immutable, IntoBytes, KnownLayout, TryFromBytes};

#[derive(Debug, TryFromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
/// Represents a packet of data that is sent over the network.
/// T is the type of data that is being sent. It must implement [`TryFromBytes`], [`IntoBytes`], [`KnownLayout`], and [`Immutable`] for efficient zero-copy ser/de.
pub struct Packet<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> {
    seq: U32,
    data: T,
}

const fn size_of_packet<T: TryFromBytes + IntoBytes + KnownLayout + Immutable>() -> usize {
    std::mem::size_of::<Packet<T>>()
}

/// A packet buffer slot. See [`RtpCircularBuffer`].
struct MaybeInitPacket<T: TryFromBytes + IntoBytes + KnownLayout + Immutable>
where
    [(); size_of_packet::<T>()]: Sized,
{
    init: bool,
    packet: [u8; size_of_packet::<T>()],
}

#[repr(C)]
/// A circular buffer of RTP packets.
/// Index into this buffer with a sequence number to get a packet.

struct RtpCircularBuffer<T: TryFromBytes + IntoBytes + KnownLayout + Immutable>
where
    [(); size_of_packet::<T>()]: Sized,
{
    earliest_seq: u32,
    buf: Box<[MaybeInitPacket<T>; 1024]>,
}

struct RecievedPacket<'a, T: TryFromBytes + IntoBytes + KnownLayout + Immutable>(
    &'a mut RtpCircularBuffer<T>,
)
where
    [(); size_of_packet::<T>()]: Sized;

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> RecievedPacket<'_, T>
where
    [(); size_of_packet::<T>()]: Sized,
{
    fn get_data(&self) -> Option<&Packet<T>> {
        let rtp_reciever = &self.0;

        if let Some(MaybeInitPacket {
            init: true,
            packet: p,
        }) = rtp_reciever.get(rtp_reciever.earliest_seq)
        {
            Some(Packet::<T>::try_ref_from_bytes(p).unwrap())
        } else {
            None
        }
    }
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> Drop for RecievedPacket<'_, T>
where
    [(); size_of_packet::<T>()]: Sized,
{
    fn drop(&mut self) {
        let rtp_reciever = &mut self.0;

        rtp_reciever
            .get_mut(rtp_reciever.earliest_seq)
            .unwrap()
            .init = false;
        rtp_reciever.earliest_seq = rtp_reciever.earliest_seq.wrapping_add(1);
    }
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> RtpCircularBuffer<T>
where
    [(); size_of_packet::<T>()]: Sized,
{
    const fn generate_default_packet() -> MaybeInitPacket<T> {
        MaybeInitPacket {
            init: false,
            packet: [0u8; size_of_packet::<T>()],
        }
    }

    fn new() -> Self {
        RtpCircularBuffer {
            earliest_seq: 0,
            buf: Box::new([const { Self::generate_default_packet() }; 1024]),
        }
    }

    fn consume_earliest_packet(&mut self) -> RecievedPacket<'_, T> {
        RecievedPacket(self)
    }

    fn get(&self, seq_num: u32) -> Option<&MaybeInitPacket<T>> {
        if seq_num.wrapping_sub(self.earliest_seq) as usize >= self.buf.len() {
            None
        } else {
            let idx = (seq_num as usize) % self.buf.len();
            Some(&self.buf[idx])
        }
    }

    fn get_mut(&mut self, seq_num: u32) -> Option<&mut MaybeInitPacket<T>> {
        if seq_num.wrapping_sub(self.earliest_seq) as usize >= self.buf.len() {
            None
        } else {
            let idx = (seq_num as usize) % self.buf.len();
            Some(&mut self.buf[idx])
        }
    }
}

pub struct RtpSender {
    sock: UdpSocket,
    seq_num: u32,
    scratch: BytesMut,
}

impl RtpSender {
    /// Create a new RTP sender.
    /// The sender will bind to the given socket and set it to non-blocking mode.

    fn new(sock: UdpSocket) -> Self {
        sock.set_nonblocking(true).unwrap();
        RtpSender {
            sock,
            seq_num: 0,
            scratch: BytesMut::with_capacity(2048),
        }
    }

    /// Send a packet over the network.

    fn send<T: IntoBytes + Immutable + ?Sized, A: AsRef<T>>(&mut self, data: A) {
        // Note that the size of the packets we use is less than 10kb, for which
        // https://www.kernel.org/doc/html/v6.3/networking/msg_zerocopy.html
        // copying is actually faster than MSG_ZEROCOPY.

        let packet = &mut self.scratch;
        packet.put_u32(self.seq_num);
        packet.put(data.as_ref().as_bytes());

        self.sock.send(packet).unwrap();
        log::debug!("Sent packet with seq: {}", self.seq_num);
        self.seq_num = self.seq_num.wrapping_add(1);
        self.scratch.clear();
    }
}

pub struct RtpReciever<T: TryFromBytes + IntoBytes + KnownLayout + Immutable>
where
    [(); size_of_packet::<T>()]: Sized,
{
    rtp_circular_buffer: Arc<Mutex<RtpCircularBuffer<T>>>,
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable + Send + 'static + Debug> RtpReciever<T>
where
    [(); size_of_packet::<T>()]: Sized,
{
    /// Launches listener thread that recieves packets and stores them in a buffer.

    fn new(sock: UdpSocket) -> Self {
        let rtp_circular_buffer = Arc::new(Mutex::new(RtpCircularBuffer::new()));

        let cloned_rtp_circular_buffer = rtp_circular_buffer.clone();
        std::thread::spawn(move || {
            accept_thread(sock, cloned_rtp_circular_buffer);
        });

        RtpReciever {
            rtp_circular_buffer,
        }
    }

    fn lock_reciever_for_consumption(&self) -> MutexGuard<'_, RtpCircularBuffer<T>> {
        self.rtp_circular_buffer.lock().unwrap()
    }
}

fn accept_thread<T: TryFromBytes + IntoBytes + KnownLayout + Immutable + Debug>(
    sock: UdpSocket,
    recv: Arc<Mutex<RtpCircularBuffer<T>>>,
) where
    [(); size_of_packet::<T>()]: Sized,
{
    sock.set_nonblocking(false).unwrap();
    log::info!("Receiver started listening on {:?}.", sock.local_addr());

    loop {
        // wait until socket has a packet to read
        let mut seq_num = [0u8; 4];
        sock.peek(&mut seq_num).unwrap();

        // we have available data to read
        let mut state = recv.lock().unwrap();

        let seq_num: u32 = U32::from_bytes(seq_num).into();

        if let Some(MaybeInitPacket { init, packet }) = state.get_mut(seq_num) {
            // Prepare a raw buffer with the known layout size of Packet<T>
            sock.recv(packet).unwrap();
            *init = true;

            log::debug!("Received packet with raw data: {:?}", packet);
        } else {
            log::info!(
                "Dropping packet with seq: {} for being too early/late; {seq_num} >= {}",
                seq_num,
                state.buf.len()
            );
            continue;
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

    // let packets queue up
    std::thread::sleep(Duration::from_secs(1));

    loop {
        let mut recv = recv.lock_reciever_for_consumption();
        if let Some(packet) = recv.consume_earliest_packet().get_data() {
            log::info!("Playing packet with seq: {}", packet.seq);
            // play audio
            // write to audio api
        }
        drop(recv);

        // sleep for a few milliseconds (2048 / 44100 - a lil bit)
        // to allow more packets to arrive
        std::thread::sleep(Duration::from_millis(45));
    }
}
