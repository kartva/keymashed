use std::{
    fmt::Debug,
    net::UdpSocket,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard}, time::Duration,
};

use bytes::{BufMut, BytesMut};
use zerocopy::byteorder::network_endian::U32;
use zerocopy::{Immutable, IntoBytes, KnownLayout, TryFromBytes};

#[derive(Debug, TryFromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
/// Represents a packet of data that is sent over the network.
/// T is the type of data that is being sent. It must implement [`TryFromBytes`], [`IntoBytes`], [`KnownLayout`], and [`Immutable`] for efficient zero-copy ser/de.
pub struct Packet<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> {
    /// [`accept_thread`] relies on the presence and type of the sequence number field.
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
pub struct MaybeInitPacket<T: TryFromBytes + IntoBytes + KnownLayout + Immutable>
where
    [(); size_of_packet::<T>()]: Sized,
{
    init: bool,
    // align to the alignment of the packet
    packet: PacketBytes<T>,
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable> MaybeInitPacket<T>
where
    [(); size_of_packet::<T>()]: Sized
{
    pub fn is_init(&self) -> bool {
        self.init
    }

    pub fn get_data(&self) -> Option<&Packet<T>> {
        if self.init {
            Some(Packet::<T>::try_ref_from_bytes(&self.packet).unwrap())
        } else {
            None
        }
    }
}

/// A circular buffer of RTP packets.
/// Index into this buffer with a sequence number to get a packet.
pub struct RtpCircularBuffer<T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BUFFER_LENGTH: usize>
where
    [(); size_of_packet::<T>()]: Sized,
{
    /// The sequence number of the earliest packet in the buffer.
    earliest_seq: u32,
    /// The span of the earliest sequence number and the latest sequence number of a received packet in the buffer.
    /// This can relied on as a hint for how full the buffer is. (i.e. how ahead is the latest received packet?)
    early_latest_span: u32,
    buf: Box<[MaybeInitPacket<T>; BUFFER_LENGTH]>,
}

/// A packet that has been received and is ready to be consumed.
/// Holds a reference to the buffer it came from. When dropped, the packet is consumed and deleted.
pub struct ReceivedPacket<'a, T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BUFFER_LENGTH: usize>(
    &'a mut RtpCircularBuffer<T, BUFFER_LENGTH>,
)
where
    [(); size_of_packet::<T>()]: Sized;

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BUFFER_LENGTH: usize> ReceivedPacket<'_, T, BUFFER_LENGTH>
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

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BUFFER_LENGTH: usize> Drop for ReceivedPacket<'_, T, BUFFER_LENGTH>
where
    [(); size_of_packet::<T>()]: Sized,
{
    fn drop(&mut self) {
        let rtp_reciever = &mut self.0;

        rtp_reciever
            .get_mut(rtp_reciever.earliest_seq)
            .unwrap()
            .init = false;
        log::trace!("consumed seq {}", rtp_reciever.earliest_seq);
        rtp_reciever.earliest_seq = rtp_reciever.earliest_seq.wrapping_add(1);
        rtp_reciever.early_latest_span = rtp_reciever.early_latest_span.saturating_sub(1);
    }
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BUFFER_LENGTH: usize> RtpCircularBuffer<T, BUFFER_LENGTH>
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
            early_latest_span: 0,
            buf: Box::new([const { Self::generate_default_packet() }; BUFFER_LENGTH]),
        }
    }

    /// Returns the slot with the earlist seq_num in the circular buffer.
    /// Note that this slot may or may not contain a packet.
    /// The slot will be consumed upon dropping the returned value.
    pub fn consume_earliest_packet(&mut self) -> ReceivedPacket<'_, T, BUFFER_LENGTH> {
        ReceivedPacket(self)
    }

    /// Returns a reference to the slotwith the earlist seq_num in the buffer.
    /// Returns None if the slot is not inhabited by a packet.
    pub fn peek_earliest_packet(&self) -> Option<&Packet<T>> {
        if let Some(MaybeInitPacket {
            init: true,
            packet: p,
        }) = self.get(self.earliest_seq)
        {
            Some(Packet::<T>::try_ref_from_bytes(p).unwrap())
        } else {
            None
        }
    }

    pub fn earliest_seq(&self) -> u32 {
        self.earliest_seq
    }

    pub fn early_latest_span(&self) -> u32 {
        self.early_latest_span
    }

    /// Returns a reference to the [`MaybeInitPacket`] slot that corresponds to the given sequence number.
    /// Returns None if the corresponding packet is not present in the buffer.
    pub fn get(&self, seq_num: u32) -> Option<&MaybeInitPacket<T>> {
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
        sock.set_nonblocking(false).unwrap();
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

        super::udp_send_retry(&self.sock, packet);
        log::trace!("sent seq: {}", self.seq_num);
        self.seq_num = self.seq_num.wrapping_add(1);
        self.scratch.clear();
    }
}

pub struct RtpReciever<T: TryFromBytes + IntoBytes + KnownLayout + Immutable, const BUFFER_LENGTH: usize>
where
    [(); size_of_packet::<T>()]: Sized,
{
    rtp_circular_buffer: Arc<Mutex<RtpCircularBuffer<T, BUFFER_LENGTH>>>,
}

impl<T: TryFromBytes + IntoBytes + KnownLayout + Immutable + Send + 'static + Debug, const BUFFER_LENGTH: usize> RtpReciever<T, BUFFER_LENGTH>
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

    /// Locks the buffer for interaction.
    pub fn lock_reciever(&self) -> MutexGuard<'_, RtpCircularBuffer<T, BUFFER_LENGTH>> {
        self.rtp_circular_buffer.lock().unwrap()
    }
}

fn accept_thread<T: TryFromBytes + IntoBytes + KnownLayout + Immutable + Debug, const BUFFER_LENGTH: usize>(
    sock: UdpSocket,
    recv: Arc<Mutex<RtpCircularBuffer<T, BUFFER_LENGTH>>>,
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

        // If the received packet has a place in the buffer, write the packet to the correct slot.
        // The received packet is allowed a place if its sequence number is larger than the earliest packet
        // by u32::MAX / 2. (If more, this is probably a late packet and we discard it.)

        if (seq_num.wrapping_sub(state.earliest_seq)) < u32::MAX / 2 {
            // If this packet will need to overwrite old existing packets.
            if seq_num.wrapping_sub(state.earliest_seq) as usize >= state.buf.len() {
                log::debug!("received an advanced packet with seq {}; dropping packets from {} to {}", seq_num, state.earliest_seq, seq_num.wrapping_sub(state.buf.len() as u32));
                while seq_num.wrapping_sub(state.earliest_seq) as usize >= state.buf.len() {
                    // Drop old packets until we can fit this new one.
                    state.consume_earliest_packet();
                }
            }
            
            state.early_latest_span = u32::max(state.early_latest_span, seq_num.wrapping_sub(state.earliest_seq));
            let MaybeInitPacket { init, packet } = state.get_mut(seq_num).expect("Circular buffer should have space for packet.");

            // Prepare a raw buffer with the known layout size of Packet<T>

            sock.recv(packet).unwrap();
            *init = true;

            if packet.len() > 16 {
                log::trace!(
                    "received seq_num {seq_num} and raw data: {:?}... (len {})", &packet[..16], packet.len()
                );
            } else {
                log::trace!(
                    "received seq_num {seq_num} and raw data: {:?}", &packet
                );
            }
        } else {
            // Otherwise, discard the packet.

            let _ = sock.recv(&mut seq_num_buffer);
            log::debug!(
                "dropping seq_num {} for being too early/late; accepted range is {}-{}",
                seq_num,
                state.earliest_seq, state.earliest_seq + state.buf.len() as u32
            );
            continue;
        }
    }
}
