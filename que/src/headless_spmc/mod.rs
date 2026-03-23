pub mod consumer;
pub mod producer;

use std::{array, mem::MaybeUninit, sync::Arc};

use consumer::Consumer;
use producer::Producer;

use crate::{Channel, LocalMode, MAGIC};

pub fn headless_pair<T: Send, const N: usize>(
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
    let consumer =
        unsafe { Consumer::join_multi_(ptr.cast(), 0, 1).unwrap() };

    unsafe {
        Arc::decrement_strong_count(ptr);
    }

    (producer, consumer)
}

pub fn headless_multi<
    T: Send,
    const N: usize,
    const NUM_CONSUMERS: usize,
>() -> (
    Producer<LocalMode, T, N>,
    [Consumer<LocalMode, T, N>; NUM_CONSUMERS],
) {
    let arc_uninit = Arc::<Channel<LocalMode, T, N>>::new_uninit();
    let ptr: *mut MaybeUninit<Channel<LocalMode, T, N>> =
        Arc::into_raw(arc_uninit).cast_mut();

    unsafe { *ptr = core::mem::zeroed() };
    let producer = unsafe {
        Producer::join_or_initialize_in_(ptr.cast()).unwrap()
    };

    let mut consumers = array::from_fn(|_| {
        MaybeUninit::<Consumer<LocalMode, T, N>>::uninit()
    });
    for i in 0..NUM_CONSUMERS {
        consumers[i].write(unsafe {
            Consumer::join_multi_(ptr.cast(), i, NUM_CONSUMERS).unwrap()
        });
    }
    unsafe {
        Arc::decrement_strong_count(ptr);
    }

    (producer, consumers.map(|m| unsafe { m.assume_init() }))
}

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
    use producer::Producer;

    use super::*;
    use crate::LocalMode;

    #[test]
    fn test_push_pop_multiple() {
        let (mut producer, mut consumer) = headless_pair::<u64, 8>();

        producer.push(69);
        producer.push(70);
        assert_eq!(consumer.pop(), None);

        producer.sync();
        assert_eq!(consumer.pop(), Some(69));
        assert_eq!(consumer.pop(), Some(70));
    }

    #[test]
    fn test_push_pop_overrun() {
        let (mut producer, mut consumer) = headless_pair::<u64, 4>();

        producer.push(69);
        producer.push(70);
        producer.push(71);
        producer.push(72);
        producer.push(73);
        producer.sync();

        // since burst_amount here is 1, we will only read last 3
        assert_eq!(consumer.pop(), Some(71));
        assert_eq!(consumer.pop(), Some(72));
        assert_eq!(consumer.pop(), Some(73));
    }

    #[test]
    fn test_multi_consumer_sequential_reads() {
        let (mut producer, [mut consumer1, mut consumer2]) =
            headless_multi::<u64, 4, 2>();
        producer.push(69);
        producer.push(70);
        producer.push(71);
        producer.push(72);
        producer.push(73);
        producer.sync();

        // since burst_amount here is 1, we will only read last 3
        assert_eq!(consumer1.pop(), Some(71));
        assert_eq!(consumer1.pop(), Some(73));

        assert_eq!(consumer2.pop(), Some(72));
    }

    #[test]
    fn test_multi_consumer_interleave_reads() {
        let (mut producer, [mut consumer1, mut consumer2]) =
            headless_multi::<u64, 4, 2>();

        producer.push(69);
        producer.push(70);
        producer.push(71);
        producer.push(72);
        producer.push(73);
        producer.sync();

        // since burst_amount here is 1, we will only read last 3
        assert_eq!(consumer1.pop(), Some(71));
        assert_eq!(consumer2.pop(), Some(72)); // This is consumer 2!
        assert_eq!(consumer1.pop(), Some(73));
    }

    #[test]
    fn test_multi_consumer_uninitialized() {
        let (mut producer, [mut consumer1, mut consumer2]) =
            headless_multi::<u64, 4, 2>();

        // Current state
        // tail = 0
        // consumer1 local head = 0
        // consumer2 local head = 1
        //
        // Both consumers should return None

        assert_eq!(consumer1.pop(), None);
        assert_eq!(consumer2.pop(), None);

        producer.push(5);
        producer.sync();
        assert_eq!(consumer2.pop(), None);
        assert_eq!(consumer1.pop(), Some(5));
        producer.push(6);
        producer.sync();
        assert_eq!(consumer1.pop(), None);
        assert_eq!(consumer2.pop(), Some(6));
        assert_eq!(consumer2.pop(), None);
    }

    #[test]
    fn test_restart_producer() {
        let (mut producer, mut consumer) = headless_pair::<u64, 4>();

        producer.push(69);
        producer.push(70);
        producer.sync();
        let spsc_ptr = (&raw mut producer);
        drop(producer); // we forget since localmode producers decrement strong count

        // Restart producer, last values should be kept
        let mut producer = unsafe {
            Producer::<LocalMode, u64, 4>::join_(
                *spsc_ptr.cast::<*mut u8>(),
            )
            .unwrap()
        };

        assert_eq!(consumer.pop(), Some(69));

        // No value published until sync
        assert_eq!(consumer.pop(), Some(70));

        // Push
        producer.push(71);
        assert_eq!(consumer.pop(), None);

        // Publish
        producer.sync();
        assert_eq!(consumer.pop(), Some(71));
    }

    #[test]
    fn test_detect_offline_consumer() {
        let (mut producer, [consumer1, consumer2]) =
            headless_multi::<u64, 4, 2>();
        assert!(!producer.consumer_heartbeat());

        consumer1.beat();
        assert!(producer.consumer_heartbeat());

        consumer2.beat();
        assert!(producer.consumer_heartbeat());

        assert!(!producer.consumer_heartbeat());
    }
}
