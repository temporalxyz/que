use std::{
    thread::JoinHandle,
    time::{Duration, Instant},
};

use que::headless_spmc::headless_multi;

fn main() {
    let (mut producer, consumers) = headless_multi::<Element, N, 10>();

    // Let's first start some message consumers.
    let mut i = 0;
    let consumers: [JoinHandle<()>; 10] =
        consumers.map(|mut consumer| {
            i += 1;
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
            producer.push(msgs);
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
