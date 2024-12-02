use bytemuck::AnyBitPattern;
use padded_atomic::CachePaddedAtomicUsize;

pub mod headless_spmc;
pub mod lossless;
pub mod padded_atomic;
pub mod page_size;

pub mod shmem;

/// Inner type shared by the producer and consumer. Supports zero copy
/// deserialization. This type is shared by both the headless/lossless
/// channel so that a user can choose to toggle backpressure when
/// restarting a system.
#[repr(C, align(128))]
pub struct Channel<T, const N: usize> {
    tail: CachePaddedAtomicUsize,
    head: CachePaddedAtomicUsize,
    producer_heartbeat: CachePaddedAtomicUsize,
    consumer_heartbeat: CachePaddedAtomicUsize,
    capacity: usize,
    magic: u64,
    buffer: [T; N],
}

/// A unique magic number identifier for the single-producer
/// single-consumer (SPSC) channel. Serves as a marker to verify
/// the integrity and type of the channel during joining and
/// initialization.
pub const MAGIC: u64 = u64::from_le_bytes(*b"TEMPORAL");

/// We use `AnyBitPattern` instead of `Pod` as it's a superset of `Pod`
unsafe impl<T: AnyBitPattern, const N: usize> Sync for Channel<T, N> {}

pub mod error {
    use crate::shmem::ShmemError;

    #[derive(Debug)]
    pub enum QueError {
        /// `MAGIC`` value was not equal to expected value.
        CorruptionDetected,

        /// Invalid amount of memory requested (i64 overflow)
        InvalidSize,

        /// Attempted to join an uninitialized channel
        Uninitialized,

        /// Channel initialized with a different capacity
        IncorrectCapacity(usize),

        /// Shared Memory Error (e.g. invalid permissions, bad file
        /// descriptor, insufficient pre-allocatedpages)
        ShmemError(ShmemError),

        /// Only used for lossless spsc
        Full,
    }

    impl From<ShmemError> for QueError {
        fn from(value: ShmemError) -> Self {
            QueError::ShmemError(value)
        }
    }

    impl std::fmt::Display for QueError {
        fn fmt(
            &self,
            f: &mut std::fmt::Formatter<'_>,
        ) -> std::fmt::Result {
            write!(f, "{:?}", self)
        }
    }

    impl std::error::Error for QueError {}
}
