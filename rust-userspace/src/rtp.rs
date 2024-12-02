use std::{
    fmt::Debug,
    net::UdpSocket,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
};

use bytes::{BufMut, BytesMut};
use zerocopy::byteorder::network_endian::U32;
use zerocopy::{Immutable, IntoBytes, KnownLayout, TryFromBytes};

#[derive(Debug, TryFromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
/// Represents a packet of data that is sent over the network.
/// T is the type of data that is being sent. It must implement [`TryFromBytes`], [`IntoBytes`], [`KnownLayout`], and [`Immutable`] for efficient zero-copy ser/de.
pub struct Packet<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> {
    pub sequence_number: U32,
    pub data: T,
}

pub const fn size_of_packet<T: TryFromBytes + IntoBytes + KnownLayout + Immutable>() -> usize {
    std::mem::size_of::<Packet<T>>()
}

/// A buffer of bytes that is the size of a packet.
/// Ensures that the buffer is correctly aligned for the packet type.
#[derive(Debug)]
struct PacketBytes<T: TryFromBytes + IntoBytes + KnownLayout + Immutable>
where
    [(); size_of_packet::<T>()]: Sized,
{
    _align: [Packet<T>; 0], // align to the alignment of the packet
    // TODO: statically assert that the alignment is correct
    inner: [u8; size_of_packet::<T>()],
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> Deref for PacketBytes<T>
where
    [(); size_of_packet::<T>()]: Sized,
{
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> DerefMut for PacketBytes<T>
where
    [(); size_of_packet::<T>()]: Sized,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// A packet buffer slot. See [`RtpCircularBuffer`].
struct MaybeInitPacket<T: TryFromBytes + IntoBytes + KnownLayout + Immutable>
where
    [(); size_of_packet::<T>()]: Sized,
{
    init: bool,
    // align to the alignment of the packet
    packet: PacketBytes<T>,
}

/// A circular buffer of RTP packets.
/// Index into this buffer with a sequence number to get a packet.
pub struct RtpCircularBuffer<T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BufferLength: usize>
where
    [(); size_of_packet::<T>()]: Sized,
{
    earliest_seq: u32,
    buf: Box<[MaybeInitPacket<T>; BufferLength]>,
}

/// A packet that has been received and is ready to be consumed.
/// Holds a reference to the buffer it came from. When dropped, the packet is consumed and deleted.
pub struct RecievedPacket<'a, T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BufferLength: usize>(
    &'a mut RtpCircularBuffer<T, BufferLength>,
)
where
    [(); size_of_packet::<T>()]: Sized;

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BufferLength: usize> RecievedPacket<'_, T, BufferLength>
where
    [(); size_of_packet::<T>()]: Sized,
{
    pub fn get_data(&self) -> Option<&Packet<T>> {
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

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BufferLength: usize> Drop for RecievedPacket<'_, T, BufferLength>
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

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BufferLength: usize> RtpCircularBuffer<T, BufferLength>
where
    [(); size_of_packet::<T>()]: Sized,
{
    const fn generate_default_packet() -> MaybeInitPacket<T> {
        MaybeInitPacket {
            init: false,
            packet: PacketBytes {
                _align: [],
                inner: [0u8; size_of_packet::<T>()],
            },
        }
    }

    fn new() -> Self {
        RtpCircularBuffer {
            earliest_seq: 0,
            buf: Box::new([const { Self::generate_default_packet() }; BufferLength]),
        }
    }

    pub fn consume_earliest_packet(&mut self) -> RecievedPacket<'_, T, BufferLength> {
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

/// An RTP sender. Sends packets of type `T` over the network.
pub struct RtpSender<T: IntoBytes + Immutable + ?Sized> {
    sock: UdpSocket,
    seq_num: u32,
    scratch: BytesMut,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: IntoBytes + Immutable + ?Sized> RtpSender<T> {
    /// Create a new RTP sender.
    /// The sender will bind to the given socket and set it to non-blocking mode.
    pub fn new(sock: UdpSocket) -> Self {
        sock.set_nonblocking(true).unwrap();
        RtpSender {
            sock,
            seq_num: 0,
            scratch: BytesMut::with_capacity(2048),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Send a packet over the network.
    pub fn send<A: AsRef<T>>(&mut self, data: A) {
        // Note that the size of the packets we use is less than 10kb, for which
        // https://www.kernel.org/doc/html/v6.3/networking/msg_zerocopy.html
        // copying is actually faster than MSG_ZEROCOPY.

        let packet = &mut self.scratch;
        packet.put_u32(self.seq_num);
        packet.put(data.as_ref().as_bytes());

        self.sock.send(packet).unwrap();
        log::debug!("{:?}: Sent packet with seq: {}", self.sock.local_addr(), self.seq_num);
        self.seq_num = self.seq_num.wrapping_add(1);
        self.scratch.clear();
    }
}

pub struct RtpReciever<T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BufferLength: usize>
where
    [(); size_of_packet::<T>()]: Sized,
{
    rtp_circular_buffer: Arc<Mutex<RtpCircularBuffer<T, BufferLength>>>,
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable + Send + 'static + Debug, const BufferLength: usize> RtpReciever<T, BufferLength>
where
    [(); size_of_packet::<T>()]: Sized,
{
    /// Launches listener thread that recieves packets and stores them in a buffer.
    pub fn new(sock: UdpSocket) -> Self {
        let rtp_circular_buffer = Arc::new(Mutex::new(RtpCircularBuffer::new()));

        let cloned_rtp_circular_buffer = rtp_circular_buffer.clone();
        std::thread::spawn(move || {
            accept_thread(sock, cloned_rtp_circular_buffer);
        });

        RtpReciever {
            rtp_circular_buffer,
        }
    }

    pub fn lock_reciever_for_consumption(&self) -> MutexGuard<'_, RtpCircularBuffer<T, BufferLength>> {
        self.rtp_circular_buffer.lock().unwrap()
    }
}

fn accept_thread<T: TryFromBytes + IntoBytes + KnownLayout + Immutable + Debug, const BufferLength: usize>(
    sock: UdpSocket,
    recv: Arc<Mutex<RtpCircularBuffer<T, BufferLength>>>,
) where
    [(); size_of_packet::<T>()]: Sized,
{
    sock.set_nonblocking(false).unwrap();
    log::info!("Receiver started listening on {:?}.", sock.local_addr());

    loop {
        // wait until socket has a packet to read
        let mut seq_num_buffer = [0u8; 4];
        sock.peek(&mut seq_num_buffer).unwrap();

        // we have available data to read
        let mut state = recv.lock().unwrap();

        let seq_num: u32 = U32::from_bytes(seq_num_buffer).into();

        // If the recieved packet has a place in the buffer, write the packet to the correct slot.
        if let Some(MaybeInitPacket { init, packet }) = state.get_mut(seq_num) {
            // Prepare a raw buffer with the known layout size of Packet<T>

            sock.recv(packet).unwrap();
            *init = true;

            if packet.len() > 16 {
                log::trace!(
                    "{:?}: with seq_num {seq_num} and raw data: {:?}...",
                    sock.local_addr(),
                    &packet[..16]
                );
            } else {
                log::trace!(
                    "{:?}: Received packet with seq_num {seq_num} and raw data: {:?}",
                    sock.local_addr(),
                    &packet
                );
            }
        } else {
            // Otherwise, discard the packet.

            let _ = sock.recv(&mut seq_num_buffer);
            log::info!(
                "{:?}: Dropping packet with seq: {} for being too early/late; accepted range is {}-{}", sock.local_addr(),
                seq_num,
                state.earliest_seq, state.earliest_seq + state.buf.len() as u32
            );
            continue;
        }
    }
}
