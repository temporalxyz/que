pub mod consumer;
pub mod producer;

use std::{mem::MaybeUninit, sync::Arc};

use crate::{
    lossless::{consumer::Consumer, producer::Producer},
    Channel, LocalMode,
};

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

pub fn lossless_pair<T: Send, const N: usize>(
) -> (Producer<LocalMode, T, N>, Consumer<LocalMode, T, N>) {
    let arc_uninit = Arc::<Channel<LocalMode, T, N>>::new_uninit();
    let ptr: *mut MaybeUninit<Channel<LocalMode, T, N>> =
        Arc::into_raw(arc_uninit).cast_mut();

    unsafe {
        *ptr = core::mem::zeroed();
    }

    let producer = unsafe {
        Producer::join_or_initialize_in_(ptr.cast()).unwrap()
    };
    let consumer = unsafe { Consumer::join(ptr.cast()).unwrap() };

    unsafe {
        Arc::decrement_strong_count(ptr);
    }

    (producer, consumer)
}

#[cfg(test)]
mod tests {
    use producer::Producer;

    use super::*;
    use crate::LocalMode;

    use std::{
        ptr::NonNull,
        sync::atomic::{AtomicU64, Ordering},
    };

    #[test]
    fn test_push_pop_multiple() {
        let (mut producer, mut consumer) = lossless_pair::<u64, 16>();

        producer.push(69).unwrap();
        producer.push(70).unwrap();
        assert_eq!(consumer.pop(), None);

        producer.sync();
        assert_eq!(consumer.pop(), Some(69));
        assert_eq!(consumer.pop(), Some(70));
    }

    #[test]
    fn test_push_pop_overrun() {
        let (mut producer, mut consumer) = lossless_pair::<u64, 4>();

        assert!(producer.push(69).is_ok());
        assert!(producer.push(70).is_ok());
        assert!(producer.push(71).is_ok());
        assert!(producer.push(72).is_ok());
        assert!(producer.push(73).is_err());

        assert_eq!(consumer.pop(), Some(69));
        assert_eq!(consumer.pop(), Some(70));
        assert_eq!(consumer.pop(), Some(71));
        assert_eq!(consumer.pop(), Some(72));

        // This should now succeed, along with next read
        assert!(producer.push(73).is_ok());
        assert_eq!(consumer.pop(), Some(73));
    }

    #[test]
    fn test_restart_producer() {
        let (mut producer, mut consumer) = lossless_pair::<u64, 16>();

        producer.push(69).unwrap();
        producer.push(70).unwrap();
        producer.sync();
        let spsc_ptr = &raw mut producer;
        drop(producer);

        // Restart producer, last values should be kept
        let mut producer = unsafe {
            Producer::<LocalMode, u64, 16>::join_(
                *spsc_ptr.cast::<*mut u8>(),
            )
            .unwrap()
        };

        assert_eq!(consumer.pop(), Some(69));
        assert_eq!(consumer.pop(), Some(70));

        // Push
        producer.push(71).unwrap();

        // No value published until sync
        assert_eq!(consumer.pop(), None);

        // Publish
        producer.sync();
        assert_eq!(consumer.pop(), Some(71));
    }

    #[test]
    fn test_detect_offline_consumer() {
        let (mut producer, consumer) = lossless_pair::<u64, 4>();
        assert!(!producer.consumer_heartbeat());

        consumer.beat();
        assert!(producer.consumer_heartbeat());

        assert!(!producer.consumer_heartbeat());
    }

    #[test]
    fn test_synchronized_metadata() {
        let (producer, consumer) = lossless_pair::<u64, 4>();

        // Start up thread to read metadata
        let read = std::thread::spawn(move || {
            let metadata: &AtomicU64 = unsafe {
                &*consumer
                    .get_padding_ptr()
                    .cast()
                    .as_ptr()
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
            (*padding_ptr.as_ptr().cast::<AtomicU64>())
                .store(metadata, Ordering::Release);
        }

        assert_eq!(read.join().unwrap(), metadata);
    }

    #[test]
    fn test_reserve_write_all_single_slice() {
        let (mut producer, mut consumer) = lossless_pair::<u8, 8>();

        let data: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8];

        // Reserve and write in one contiguous slice
        let mut reservation = producer.reserve(data.len()).unwrap();
        reservation.write_all(data);
        reservation.commit();

        // Verify all data is readable
        for &expected in data {
            assert_eq!(consumer.pop(), Some(expected));
        }
        assert_eq!(consumer.pop(), None);
    }

    #[test]
    fn test_reserve_write_all_wraparound() {
        let (mut producer, mut consumer) = lossless_pair::<u8, 8>();

        // Fill buffer near the end
        for i in 0..6 {
            producer.push(i).unwrap();
        }
        producer.sync();

        // Consume first 4 elements to make space at the beginning
        for i in 0..4 {
            assert_eq!(consumer.pop(), Some(i));
        }

        // Now we have 2 elements at positions 4,5 and space for 6 more
        // Writing 6 elements will use positions 6,7,0,1,2,3 (wraparound)
        let data: &[u8] = &[10, 11, 12, 13, 14, 15];

        let mut reservation = producer.reserve(data.len()).unwrap();
        reservation.write_all(data);
        reservation.commit();

        // Read remaining old elements
        assert_eq!(consumer.pop(), Some(4));
        assert_eq!(consumer.pop(), Some(5));

        // Read new elements (that wrapped around)
        for &expected in data {
            assert_eq!(consumer.pop(), Some(expected));
        }
        assert_eq!(consumer.pop(), None);
    }

    #[test]
    fn test_reserve_no_capacity() {
        let (mut producer, mut consumer) = lossless_pair::<u8, 4>();

        // Fill the buffer completely
        producer.push(1).unwrap();
        producer.push(2).unwrap();
        producer.push(3).unwrap();
        producer.push(4).unwrap();

        // Try to reserve space - should fail
        let data: &[u8] = &[5, 6];
        assert!(producer.reserve(data.len()).is_err());

        // Consume one element to make space
        assert_eq!(consumer.pop(), Some(1));

        // Now reservation should succeed
        let mut reservation = producer.reserve(1).unwrap();
        reservation.write_next(5);
        reservation.commit();

        assert_eq!(consumer.pop(), Some(2));
        assert_eq!(consumer.pop(), Some(3));
        assert_eq!(consumer.pop(), Some(4));
        assert_eq!(consumer.pop(), Some(5));
        assert_eq!(consumer.pop(), None);
    }
}
