pub mod consumer;
pub mod producer;

use std::{mem::MaybeUninit, sync::Arc};

use consumer::Consumer;
use producer::Producer;

use crate::{Channel, LocalMode, MAGIC};

pub fn headless_pair<T: Send, const N: usize>(
) -> (Producer<LocalMode, T, N>, Consumer<LocalMode, T, N>) {
    let arc_uninit = Arc::<Channel<LocalMode, T, N>>::new_uninit();
    let ptr: *mut MaybeUninit<Channel<LocalMode, T, N>> =
        Arc::into_raw(arc_uninit).cast_mut();
    unsafe { *ptr = core::mem::zeroed() };

    let producer =
        unsafe { Producer::join_or_initialize_in(ptr.cast()).unwrap() };
    let consumer = unsafe { Consumer::join(ptr.cast()).unwrap() };

    (producer, consumer)
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
    use consumer::Consumer;
    use producer::Producer;

    use super::*;
    use crate::{
        utils::{new_spsc_buffer, Alloc},
        LocalMode,
    };

    /// This leaks! Only for tests!
    pub(crate) fn new_spsc_pair<T: Send, const N: usize>(
    ) -> (Alloc, Producer<LocalMode, T, N>, Consumer<LocalMode, T, N>)
    {
        let alloc = new_spsc_buffer::<T, N>();

        let producer = unsafe {
            Producer::join_or_initialize_in(alloc.ptr).unwrap()
        };
        let consumer = unsafe { Consumer::join(alloc.ptr).unwrap() };

        (alloc, producer, consumer)
    }

    #[test]
    fn test_push_pop_multiple() {
        let (_alloc, mut producer, mut consumer) =
            new_spsc_pair::<u64, 8>();

        producer.push(69);
        producer.push(70);
        assert_eq!(consumer.pop(), None);

        producer.sync();
        assert_eq!(consumer.pop(), Some(69));
        assert_eq!(consumer.pop(), Some(70));
    }

    #[test]
    fn test_push_pop_overrun() {
        let (_alloc, mut producer, mut consumer) =
            new_spsc_pair::<u64, 4>();

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
        let alloc = new_spsc_buffer::<u64, 4>();
        let mut producer: Producer<LocalMode, u64, 4> = unsafe {
            Producer::join_or_initialize_in(alloc.ptr).unwrap()
        };
        let mut consumer1: Consumer<LocalMode, u64, 4> =
            unsafe { Consumer::join_multi(alloc.ptr, 2).unwrap() };
        let mut consumer2 = consumer1.next_multi().unwrap();

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
        let alloc = new_spsc_buffer::<u64, 4>();
        let mut producer: Producer<LocalMode, u64, 4> = unsafe {
            Producer::join_or_initialize_in(alloc.ptr).unwrap()
        };
        let mut consumer1: Consumer<LocalMode, u64, 4> =
            unsafe { Consumer::join_multi(alloc.ptr, 2).unwrap() };
        let mut consumer2 = consumer1.next_multi().unwrap();

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
        let alloc = new_spsc_buffer::<u64, 4>();
        let mut producer: Producer<LocalMode, u64, 4> = unsafe {
            Producer::join_or_initialize_in(alloc.ptr).unwrap()
        };
        let mut consumer1: Consumer<LocalMode, u64, 4> =
            unsafe { Consumer::join_multi(alloc.ptr, 2).unwrap() };
        let mut consumer2 = consumer1.next_multi().unwrap();

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
        let alloc = new_spsc_buffer::<u64, 4>();
        let mut producer: Producer<LocalMode, u64, 4> = unsafe {
            Producer::join_or_initialize_in(alloc.ptr).unwrap()
        };
        let mut consumer: Consumer<LocalMode, u64, 4> =
            unsafe { Consumer::join(alloc.ptr).unwrap() };

        producer.push(69);
        producer.push(70);
        producer.sync();
        drop(producer);

        // Restart producer, last values should be kept
        let mut producer = unsafe {
            Producer::<LocalMode, u64, 4>::join(alloc.ptr).unwrap()
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
        let alloc = new_spsc_buffer::<u64, 4>();
        let mut producer: Producer<LocalMode, u64, 4> = unsafe {
            Producer::join_or_initialize_in(alloc.ptr).unwrap()
        };
        assert!(!producer.consumer_heartbeat());

        let consumer1: Consumer<LocalMode, u64, 4> =
            unsafe { Consumer::join_multi(alloc.ptr, 2).unwrap() };
        consumer1.beat();
        assert!(producer.consumer_heartbeat());

        let consumer2: Consumer<LocalMode, u64, 4> =
            consumer1.next_multi().unwrap();
        consumer2.beat();
        assert!(producer.consumer_heartbeat());

        assert!(!producer.consumer_heartbeat());
    }
}
