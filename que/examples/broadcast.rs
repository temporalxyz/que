use std::{
    alloc::Layout,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use bytemuck::AnyBitPattern;
use que::{
    headless_spmc::{consumer::Consumer, producer::Producer},
    Channel,
};

fn main() {
    // Allocate memory shared by consumers/producer
    let buffer = new_spsc_buffer::<Element, N>();
    let mut producer = unsafe {
        Producer::<Element, N>::initialize_in(buffer).unwrap()
    };

    // Let's first start some message consumers.
    let consumers: [JoinHandle<()>; 10] = std::array::from_fn(|i| {
        let mut consumer =
            unsafe { Consumer::<Element, N>::join(buffer).unwrap() };
        std::thread::spawn(move || {
            let mut expected = 0;
            while expected < NUM_MSGS {
                if let Some(msg) = consumer.pop() {
                    assert_eq!(expected, msg);
                    expected += 1;
                }
            }
            println!("consumer {i} read {NUM_MSGS} messages");
        })
    });

    // Solana mainnet-beta is on the order of O(10,000) TPS.
    // That's one message every 10 micros.
    // Let's produce 10,000 msgs at about that pace (1s)
    let timer = Instant::now();
    let mut msgs = 0;
    while msgs < NUM_MSGS {
        if timer.elapsed() > Duration::from_micros(10 * msgs) {
            producer.push(&msgs);
            producer.sync();
            msgs += 1;
        }
    }
    for consumer in consumers {
        consumer.join().unwrap()
    }
}

type Element = u64;
const N: usize = 1024;
const NUM_MSGS: u64 = 10_000;

fn new_spsc_buffer<T: AnyBitPattern, const N: usize>() -> *mut u8 {
    let buffer_size = size_of::<Channel<T, N>>();
    let layout = Layout::from_size_align(buffer_size, 128).unwrap();

    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        panic!("alloc failed")
    }
    ptr
}
