//! An implementation for an RTP-like protocol. Has a strong emphasis on zero-copy ser/de of packets using [`zerocopy`] (because figuring it out was fun).
//!
//! Nomenclature:
//! - Payload: The type of the data that is being sent.
//! - Packet: A packet of data that is sent over the network. It contains a header and the payload data.
//!
//! Payloads are allowed to be unsized. However, since we maintain a buffer of packets, we must still know an upper-bound on the size of the payload.
//! The `SLOT_SIZE` parameter in the types in this module represents this upper-bound. (Note that it is exclusive of packet metadata)
//!
//! Because unsized types do not have a fixed alignment, the types in this module have a type parameter `AlignPayloadTo` that represents a type that has the correct alignment for the payload.
//! - Slices of data `[T]` have alignment of `T` (`Slice` variants of the structs in this module encode this concept).
//! - For `dyn Trait` objects, use the type that the dyn was derived from.
//!
//! Read more about alignment in Rust [here](https://doc.rust-lang.org/reference/type-layout.html).

use std::{
    fmt::Debug,
    marker::PhantomData,
    mem::offset_of,
    net::UdpSocket,
    num::NonZero,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
};

use zerocopy::{byteorder::network_endian::U32, FromBytes, Unaligned};
use zerocopy::{Immutable, IntoBytes, KnownLayout, TryFromBytes};

#[derive(Debug, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct PacketHeader {
    sequence_number: U32,
}

#[derive(Debug, TryFromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
/// Represents a packet of data that is sent over the network.
/// T is the type of data that is being sent. It must implement [`TryFromBytes`], [`IntoBytes`], [`KnownLayout`], and [`Immutable`] for efficient zero-copy ser/de.
pub struct Packet<Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized> {
    /// [`accept_thread`] relies on the presence and type of the sequence number field.
    pub header: PacketHeader,
    pub data: Payload,
}

pub const fn size_of_packet<Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable>() -> usize {
    std::mem::size_of::<Packet<Payload>>()
}

/// A buffer of bytes that is the size of a packet.
///
/// The buffer is aligned for a packet with a payload of `PayloadAlignTo`.
/// The `SLOT_SIZE` is the size of the packet slot in bytes. This size **is not inclusive** of packet metadata.
struct AlignedPacketBytes<
    Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
    AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
    const SLOT_SIZE: usize,
> where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    _phantom: PhantomData<Payload>,
    _align: [Packet<AlignPayloadTo>; 0], // align to the alignment of the packet
    // TODO: statically assert that the alignment is correct
    inner: [u8; size_of_packet::<[u8; SLOT_SIZE]>()],
}

impl<
        Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
        AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
        const SLOT_SIZE: usize,
    > Debug for AlignedPacketBytes<Payload, AlignPayloadTo, SLOT_SIZE>
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PacketBytes")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<
        Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
        AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
        const SLOT_SIZE: usize,
    > Deref for AlignedPacketBytes<Payload, AlignPayloadTo, SLOT_SIZE>
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<
        Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
        AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
        const SLOT_SIZE: usize,
    > DerefMut for AlignedPacketBytes<Payload, AlignPayloadTo, SLOT_SIZE>
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// A packet buffer slot. See [`RtpCircularBuffer`].
/// The `PACKET_SLOT_SIZE` is the size of the packet slot in bytes. This size **is not inclusive** of packet metadata.
pub struct MaybeInitPacket<
    Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
    AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
    const SLOT_SIZE: usize,
> where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    /// Size of the received packet. Is None if the packet is not initialized.
    recv_size: Option<NonZero<usize>>,
    // align to the alignment of the packet
    packet: AlignedPacketBytes<Payload, AlignPayloadTo, SLOT_SIZE>,
}

impl<
        Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
        AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
        const SLOT_SIZE: usize,
    > MaybeInitPacket<Payload, AlignPayloadTo, SLOT_SIZE>
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    pub fn is_init(&self) -> bool {
        self.recv_size.is_some()
    }

    pub fn get_data(&self) -> Option<&Packet<Payload>> {
        if let Some(len) = self.recv_size {
            Some(Packet::<Payload>::try_ref_from_bytes(&self.packet[..len.into()]).unwrap())
        } else {
            None
        }
    }
}

/// A circular buffer of RTP packets.
/// Index into this buffer with a sequence number to get a packet.
/// `SLOT_SIZE` is the size of the payload data in bytes. This size is exclusive of packet metadata.
pub struct RtpCircularBuffer<
    Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
    AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
    const SLOT_SIZE: usize,
    const BUFFER_LENGTH: usize,
> where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    /// The sequence number of the earliest packet in the buffer.
    earliest_seq: u32,
    /// The span of the earliest sequence number and the latest sequence number of a received packet in the buffer.
    /// This can relied on as a hint for how full the buffer is. (i.e. how ahead is the latest received packet?)
    early_latest_span: u32,
    buf: Box<[MaybeInitPacket<Payload, AlignPayloadTo, SLOT_SIZE>; BUFFER_LENGTH]>,
}

/// A packet that has been received and is ready to be consumed.
/// Holds a reference to the buffer it came from. When dropped, the packet is consumed and deleted.
pub struct ReceivedPacket<
    'a,
    Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
    AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
    const SLOT_SIZE: usize,
    const BUFFER_LENGTH: usize,
>(&'a mut RtpCircularBuffer<Payload, AlignPayloadTo, SLOT_SIZE, BUFFER_LENGTH>)
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized;

impl<
        Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
        AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
        const SLOT_SIZE: usize,
        const BUFFER_LENGTH: usize,
    > ReceivedPacket<'_, Payload, AlignPayloadTo, SLOT_SIZE, BUFFER_LENGTH>
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    pub fn get_data(&self) -> Option<&Packet<Payload>> {
        let rtp_receiver = &self.0;

        if let Some(MaybeInitPacket {
            recv_size: Some(packet_len),
            packet: p,
            ..
        }) = rtp_receiver.get(rtp_receiver.earliest_seq)
        {
            log::trace!("Getting data from seq {} with len {}", rtp_receiver.earliest_seq, packet_len);
            Some(Packet::<Payload>::try_ref_from_bytes(&p[..((*packet_len).into())]).unwrap())
        } else {
            None
        }
    }
}

impl<
        Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
        AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
        const SLOT_SIZE: usize,
        const BUFFER_LENGTH: usize,
    > Drop for ReceivedPacket<'_, Payload, AlignPayloadTo, SLOT_SIZE, BUFFER_LENGTH>
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    fn drop(&mut self) {
        let rtp_receiver = &mut self.0;

        rtp_receiver
            .get_mut(rtp_receiver.earliest_seq)
            .unwrap()
            .recv_size = None;
        log::trace!("consumed seq {}", rtp_receiver.earliest_seq);
        rtp_receiver.earliest_seq = rtp_receiver.earliest_seq.wrapping_add(1);
        rtp_receiver.early_latest_span = rtp_receiver.early_latest_span.saturating_sub(1);
    }
}

impl<
        Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
        AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
        const SLOT_SIZE: usize,
        const BUFFER_LENGTH: usize,
    > RtpCircularBuffer<Payload, AlignPayloadTo, SLOT_SIZE, BUFFER_LENGTH>
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    const fn generate_default_packet() -> MaybeInitPacket<Payload, AlignPayloadTo, SLOT_SIZE> {
        MaybeInitPacket {
            recv_size: None,
            packet: AlignedPacketBytes {
                _phantom: PhantomData,
                _align: [],
                inner: [0u8; size_of_packet::<[u8; SLOT_SIZE]>()],
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

    /// Returns the slot with the earliest seq_num in the circular buffer.
    /// Note that this slot may or may not contain a packet.
    /// The slot will be consumed upon dropping the returned value.
    pub fn consume_earliest_packet(
        &mut self,
    ) -> ReceivedPacket<'_, Payload, AlignPayloadTo, SLOT_SIZE, BUFFER_LENGTH> {
        ReceivedPacket(self)
    }

    /// Returns a reference to the slot with the earliest seq_num in the buffer.
    /// Returns None if the slot is not inhabited by a packet.
    pub fn peek_earliest_packet(&self) -> Option<&Packet<Payload>> {
        self.get(self.earliest_seq).and_then(|p| p.get_data())
    }

    pub fn earliest_seq(&self) -> u32 {
        self.earliest_seq
    }

    pub fn early_latest_span(&self) -> u32 {
        self.early_latest_span
    }

    /// Returns a reference to the [`MaybeInitPacket`] slot that corresponds to the given sequence number.
    /// Returns None if the corresponding packet is not present in the buffer.
    pub fn get(&self, seq_num: u32) -> Option<&MaybeInitPacket<Payload, AlignPayloadTo, SLOT_SIZE>> {
        if seq_num.wrapping_sub(self.earliest_seq) as usize >= self.buf.len() {
            None
        } else {
            let idx = (seq_num as usize) % self.buf.len();
            Some(&self.buf[idx])
        }
    }

    fn get_mut(
        &mut self,
        seq_num: u32,
    ) -> Option<&mut MaybeInitPacket<Payload, AlignPayloadTo, SLOT_SIZE>> {
        if seq_num.wrapping_sub(self.earliest_seq) as usize >= self.buf.len() {
            None
        } else {
            let idx = (seq_num as usize) % self.buf.len();
            Some(&mut self.buf[idx])
        }
    }
}

pub type RtpSizedPayloadSender<Payload: TryFromBytes + IntoBytes + Immutable + KnownLayout> =
    RtpSender<Payload, Payload, { size_of::<Payload>() }>;

pub type RtpSlicePayloadSender<
    SlicedPayload: TryFromBytes + IntoBytes + Immutable + KnownLayout,
    const MAX_SLICE_LENGTH: usize,
> = RtpSender<[SlicedPayload], SlicedPayload, { size_of::<SlicedPayload>() * MAX_SLICE_LENGTH }>;

/// An RTP sender that sends packets over the network.
pub struct RtpSender<
    Payload: TryFromBytes + IntoBytes + Immutable + KnownLayout + ?Sized,
    AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
    const SLOT_SIZE: usize,
> where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    sock: UdpSocket,
    seq_num: u32,
    /// A correctly aligned scratch buffer for writing packet data to.
    scratch: AlignedPacketBytes<Payload, AlignPayloadTo, SLOT_SIZE>,
}

impl<
        Payload: TryFromBytes + IntoBytes + Immutable + KnownLayout + ?Sized,
        AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
        const SLOT_SIZE: usize,
    > RtpSender<Payload, AlignPayloadTo, SLOT_SIZE>
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    /// Create a new RTP sender.
    /// The sender will bind to the given socket.
    /// The sender will use a scratch buffer of size `max_size` for packet serialization.
    pub fn new(sock: UdpSocket) -> Self {
        RtpSender {
            sock,
            seq_num: 0,
            scratch: AlignedPacketBytes {
                _phantom: PhantomData,
                _align: [],
                inner: [0u8; size_of_packet::<[u8; SLOT_SIZE]>()],
            },
        }
    }

    /// Get the seq num of the next packet to be sent.
    pub fn seq_num(&self) -> u32 {
        self.seq_num
    }

    /// Send a packet over the network by filling data in the mutable slice.
    /// The closure `fill` is called with a mutable slice of the packet data, and should return the number of bytes to be sent.
    pub fn send_bytes<'a>(&'a mut self, fill: impl FnOnce(&mut [u8]) -> usize) {
        // Note that the size of the packets we use is less than 10kb, for which
        // https://www.kernel.org/doc/html/v6.3/networking/msg_zerocopy.html
        // copying is actually faster than MSG_ZEROCOPY.

        let packet = &mut self.scratch;

        let header =
            PacketHeader::mut_from_bytes(&mut packet[0..size_of::<PacketHeader>()]).unwrap();

        header.sequence_number = self.seq_num.into();
        
        // Note that this is only correct because the alignment of the packet is the same as the alignment of the payload.
        // Also #[repr(C)] on Packet should guarantee some amount of stability wrt. padding.
        
        let packet_start_offset = offset_of!(Packet<AlignPayloadTo>, data);
        let mem = &mut packet[packet_start_offset..];
        let payload_len = fill(mem);
        
        super::udp_send(&self.sock, &packet[..packet_start_offset + payload_len]);
        log::trace!("sent seq: {} ({} bytes)", self.seq_num, packet_start_offset + payload_len);
        
        self.seq_num = self.seq_num.wrapping_add(1);
    }
}

/// Implementation for Payloads that can be interpreted from the raw byte buffer without further validation.
/// This enables creating a &mut Payload from the internal byte buffer.

// TODO: figure out creating a MaybeUninit<Payload> from the internal byte buffer to give to the closure.
// MaybeUninit does not implement IntoBytes and thus creating a mutable reference to it from the internal byte buffer is not possible.

impl<
        Payload: FromBytes + TryFromBytes + IntoBytes + Immutable + KnownLayout,
        AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
        const SLOT_SIZE: usize,
    > RtpSender<Payload, AlignPayloadTo, SLOT_SIZE>
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    /// Send a packet over the network by filling data in the mutable slice.
    /// The closure `fill` is called with a mutable reference of the data.
    pub fn send<'a>(&'a mut self, fill: impl FnOnce(&mut Payload)) {
        self.send_bytes(|mem| {
            let mut data = Payload::mut_from_bytes(mem).unwrap();
            fill(&mut data);
            size_of_val(&data)
        });
    }
}

/// An RTP receiver that recieves packets over the network.
pub struct RtpReceiver<
    Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + ?Sized,
    AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
    const SLOT_SIZE: usize,
    const BUFFER_LENGTH: usize,
> where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    rtp_circular_buffer: Arc<Mutex<RtpCircularBuffer<Payload, AlignPayloadTo, SLOT_SIZE, BUFFER_LENGTH>>>,
}

pub type RtpSizedPayloadReceiver<
    Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable,
    const BUFFER_LENGTH: usize,
> = RtpReceiver<Payload, Payload, { size_of::<Payload>() }, BUFFER_LENGTH>;

pub type RtpSlicePayloadReceiver<
    SlicedPayload: TryFromBytes + IntoBytes + KnownLayout + Immutable,
    const MAX_SLICE_LENGTH: usize,
    const BUFFER_LENGTH: usize,
> = RtpReceiver<
    [SlicedPayload],
    SlicedPayload,
    { size_of::<SlicedPayload>() * MAX_SLICE_LENGTH },
    BUFFER_LENGTH,
>;

impl<
        Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + Send + 'static + Debug + ?Sized,
        AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable + Send + 'static + Debug,
        const SLOT_SIZE: usize,
        const BUFFER_LENGTH: usize,
    > RtpReceiver<Payload, AlignPayloadTo, SLOT_SIZE, BUFFER_LENGTH>
where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
{
    /// Launches listener thread that recieves packets and stores them in a buffer.
    pub fn new(sock: UdpSocket) -> Self {
        let rtp_circular_buffer = Arc::new(Mutex::new(RtpCircularBuffer::new()));

        let cloned_rtp_circular_buffer = rtp_circular_buffer.clone();
        std::thread::spawn(move || {
            accept_thread(sock, cloned_rtp_circular_buffer);
        });

        RtpReceiver {
            rtp_circular_buffer,
        }
    }

    /// Locks the buffer for interaction.
    pub fn lock_receiver(
        &self,
    ) -> MutexGuard<'_, RtpCircularBuffer<Payload, AlignPayloadTo, SLOT_SIZE, BUFFER_LENGTH>> {
        self.rtp_circular_buffer.lock().unwrap()
    }
}

fn accept_thread<
    Payload: TryFromBytes + IntoBytes + KnownLayout + Immutable + Debug + ?Sized,
    AlignPayloadTo: TryFromBytes + IntoBytes + KnownLayout + Immutable,
    const SLOT_SIZE: usize,
    const BUFFER_LENGTH: usize,
>(
    sock: UdpSocket,
    recv: Arc<Mutex<RtpCircularBuffer<Payload, AlignPayloadTo, SLOT_SIZE, BUFFER_LENGTH>>>,
) where
    [(); size_of_packet::<[u8; SLOT_SIZE]>()]: Sized,
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
                log::debug!(
                    "received an advanced packet with seq {}; dropping packets from {} to {}",
                    seq_num,
                    state.earliest_seq,
                    seq_num.wrapping_sub(state.buf.len() as u32)
                );
                while seq_num.wrapping_sub(state.earliest_seq) as usize >= state.buf.len() {
                    // Drop old packets until we can fit this new one.
                    state.consume_earliest_packet();
                }
            }

            state.early_latest_span = u32::max(
                state.early_latest_span,
                seq_num.wrapping_sub(state.earliest_seq),
            );
            let MaybeInitPacket {
                recv_size: init,
                packet,
                ..
            } = state
                .get_mut(seq_num)
                .expect("Circular buffer should have space for packet.");

            let len = sock.recv(packet).unwrap();
            *init = Some(NonZero::new(len).expect("Packet should have non-zero length."));

            if len > 16 {
                log::trace!(
                    "received seq_num {seq_num} and raw data: {:?}... (len {})",
                    &packet[..16],
                    len
                );
            } else {
                log::trace!("received seq_num {seq_num} and raw data: {:?}", &packet);
            }
        } else {
            // Otherwise, discard the packet.

            let _ = sock.recv(&mut seq_num_buffer);
            log::debug!(
                "dropping seq_num {} for being too early/late; accepted range is {}-{}",
                seq_num,
                state.earliest_seq,
                state.earliest_seq + state.buf.len() as u32
            );
            continue;
        }
    }
}
