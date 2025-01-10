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
    use std::{
        alloc::Layout,
        mem::size_of,
        ptr::NonNull,
        sync::atomic::{AtomicU64, Ordering},
    };

    /// This leaks! Only for tests!
    fn new_spsc_buffer<T: AnyBitPattern, const N: usize>() -> *mut u8 {
        let buffer_size = size_of::<Channel<T, N>>();
        let layout = Layout::from_size_align(buffer_size, 128).unwrap();

        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            panic!("alloc failed during test")
        }
        ptr
    }

    fn new_spsc_pair<T: AnyBitPattern, const N: usize>(
    ) -> (Producer<T, N>, Consumer<T, N>) {
        let buffer = new_spsc_buffer::<T, N>();

        let producer =
            unsafe { Producer::initialize_in(buffer).unwrap() };
        let consumer = unsafe { Consumer::join(buffer).unwrap() };

        (producer, consumer)
    }

    #[test]
    fn test_push_pop_multiple() {
        let (mut producer, mut consumer) = new_spsc_pair::<u64, 8>();

        producer.push(&69).unwrap();
        producer.push(&70).unwrap();
        assert_eq!(consumer.pop(), None);

        producer.sync();
        assert_eq!(consumer.pop(), Some(69));
        assert_eq!(consumer.pop(), Some(70));
    }

    #[test]
    fn test_push_pop_overrun() {
        let (mut producer, mut consumer) = new_spsc_pair::<u64, 4>();

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
        let buffer = new_spsc_buffer::<u64, 4>();
        let mut producer: Producer<u64, 4> =
            unsafe { Producer::initialize_in(buffer).unwrap() };
        let mut consumer: Consumer<u64, 4> =
            unsafe { Consumer::join(buffer).unwrap() };

        producer.push(&69).unwrap();
        producer.push(&70).unwrap();
        producer.sync();
        drop(producer);

        // Restart producer, last values should be kept
        let mut producer =
            unsafe { Producer::<u64, 4>::join(buffer).unwrap() };

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
        let buffer = new_spsc_buffer::<u64, 4>();
        let mut producer: Producer<u64, 4> =
            unsafe { Producer::initialize_in(buffer).unwrap() };
        assert!(!producer.consumer_heartbeat());

        let consumer: Consumer<u64, 4> =
            unsafe { Consumer::join(buffer).unwrap() };
        consumer.beat();
        assert!(producer.consumer_heartbeat());

        assert!(!producer.consumer_heartbeat());
    }

    #[test]
    fn test_synchronized_metadata() {
        struct SendPtr(NonNull<u8>);
        unsafe impl Send for SendPtr {}

        let buffer =
            SendPtr(NonNull::new(new_spsc_buffer::<u64, 4>()).unwrap());
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
