//! TODO: audit

pub mod consumer;
pub mod producer;

use crate::Channel;

const fn burst_amount<const N: usize>() -> usize {
    // Producer can write up to 1/4 of the buffer at a time
    const BURST_DENOM: usize = 4;

    let x = N / BURST_DENOM;

    if x == 0 {
        1
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use bytemuck::AnyBitPattern;
    use consumer::Consumer;
    use producer::Producer;

    use super::*;
    use crate::test_utils::{new_spsc_buffer, Alloc};

    use std::{
        ptr::NonNull,
        sync::atomic::{AtomicU64, Ordering},
    };

    pub(crate) fn new_spsc_pair<T: AnyBitPattern, const N: usize>(
    ) -> (Alloc, Producer<T, N>, Consumer<T, N>) {
        let alloc = new_spsc_buffer::<T, N>();

        let producer =
            unsafe { Producer::initialize_in(alloc.ptr).unwrap() };
        let consumer = unsafe { Consumer::join(alloc.ptr).unwrap() };

        (alloc, producer, consumer)
    }

    #[test]
    fn test_push_pop_multiple() {
        let (_alloc, mut producer, mut consumer) =
            new_spsc_pair::<u64, 8>();

        producer.push(&69).unwrap();
        producer.push(&70).unwrap();
        assert_eq!(consumer.pop(), None);

        producer.sync();
        assert_eq!(consumer.pop(), Some(69));
        assert_eq!(consumer.pop(), Some(70));
    }

    #[test]
    fn test_push_pop_overrun() {
        let (_alloc, mut producer, mut consumer) =
            new_spsc_pair::<u64, 4>();

        assert!(producer.push(&69).is_ok());
        assert!(producer.push(&70).is_ok());
        assert!(producer.push(&71).is_ok());
        assert!(producer.push(&72).is_ok());
        assert!(producer.push(&73).is_err());

        // since burst_amount here is 1, we will only read last 3
        assert_eq!(consumer.pop(), Some(69));
        assert_eq!(consumer.pop(), Some(70));
        assert_eq!(consumer.pop(), Some(71));
    }

    #[test]
    fn test_restart_producer() {
        let alloc = new_spsc_buffer::<u64, 4>();
        let mut producer: Producer<u64, 4> =
            unsafe { Producer::initialize_in(alloc.ptr).unwrap() };
        let mut consumer: Consumer<u64, 4> =
            unsafe { Consumer::join(alloc.ptr).unwrap() };

        producer.push(&69).unwrap();
        producer.push(&70).unwrap();
        producer.sync();
        drop(producer);

        // Restart producer, last values should be kept
        let mut producer =
            unsafe { Producer::<u64, 4>::join(alloc.ptr).unwrap() };

        assert_eq!(consumer.pop(), Some(69));

        // No value published until sync
        assert_eq!(consumer.pop(), Some(70));

        // Push
        producer.push(&71).unwrap();
        assert_eq!(consumer.pop(), None);

        // Publish
        producer.sync();
        assert_eq!(consumer.pop(), Some(71));
    }

    #[test]
    fn test_detect_offline_consumer() {
        let alloc = new_spsc_buffer::<u64, 4>();
        let mut producer: Producer<u64, 4> =
            unsafe { Producer::initialize_in(alloc.ptr).unwrap() };
        assert!(!producer.consumer_heartbeat());

        let consumer: Consumer<u64, 4> =
            unsafe { Consumer::join(alloc.ptr).unwrap() };
        consumer.beat();
        assert!(producer.consumer_heartbeat());

        assert!(!producer.consumer_heartbeat());
    }

    #[test]
    fn test_synchronized_metadata() {
        struct SendPtr(NonNull<u8>);
        unsafe impl Send for SendPtr {}

        let alloc = new_spsc_buffer::<u64, 4>();
        let buffer = SendPtr(NonNull::new(alloc.ptr).unwrap());
        let producer: Producer<u64, 4> = unsafe {
            Producer::initialize_in(buffer.0.as_ptr()).unwrap()
        };

        // Start up thread to read metadata
        let read = std::thread::spawn(move || {
            let buffer = buffer;
            let consumer: Consumer<u64, 4> =
                unsafe { Consumer::join(buffer.0.as_ptr()).unwrap() };
            let metadata: &AtomicU64 = unsafe {
                consumer
                    .get_padding_ptr()
                    .cast()
                    .as_ref()
            };
            loop {
                let value = metadata.load(Ordering::Acquire);
                if value != 0 {
                    return value;
                }
            }
        });

        // Set metadata (padding is 128-byte aligned)
        let metadata: u64 = 12345_u64;
        let padding_ptr: NonNull<[u8; 112]> =
            producer.get_padding_ptr();
        unsafe {
            (padding_ptr.cast::<AtomicU64>())
                .as_ref()
                .store(metadata, Ordering::Release);
        }

        assert_eq!(read.join().unwrap(), metadata);
    }
}
