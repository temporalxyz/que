use std::sync::atomic::{AtomicU64, AtomicUsize};

use bytemuck::AnyBitPattern;
use padded_atomic::CachePaddedAtomicUsize;

pub mod headless_spmc;
pub mod lossless;
pub mod padded_atomic;
pub mod page_size;

pub mod shmem;

#[cfg(test)]
pub(crate) mod test_utils;

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
    padding: [u8; 128 - 16],
    capacity: AtomicUsize,
    magic: AtomicU64,
    buffer: [T; N],
}

impl<T, const N: usize> Channel<T, N> {
    #[rustfmt::skip]
    pub fn print_layout() {
        println!("Channel::<{}, {N}> Layout", core::any::type_name::<T>());
        println!("tail offset:               {}", core::mem::offset_of!(Self, tail));
        println!("head offset:               {}", core::mem::offset_of!(Self, head));
        println!("producer_heartbeat offset: {}", core::mem::offset_of!(Self, producer_heartbeat));
        println!("consumer_heartbeat offset: {}", core::mem::offset_of!(Self, consumer_heartbeat));
        println!("padding offset:            {}", core::mem::offset_of!(Self, padding));
        println!("capacity offset:           {}", core::mem::offset_of!(Self, capacity));
        println!("magic offset:              {}", core::mem::offset_of!(Self, magic));
        println!("buffer offset:             {}", core::mem::offset_of!(Self, buffer));
    }
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
