//! Loom permutation tests for the lossless SPSC queue (`ShmemMode`, stack buffer).
//!
//! `lossless::tests` / `headless_spmc::tests` are not run with `--features loom`:
//! `LocalMode` keeps the channel in `std::sync::Arc`, and drops run after a
//! `loom::model` closure would return, which would touch Loom atomics outside
//! the model. These tests use a stack buffer and `ShmemMode` instead.
//!
//! The old per-push `sync` stress test was removed: it overlapped
//! `batched_pushes` / `fill_drain_refill` but exploded Loom’s search space.
//!
//! ```text
//! cargo test -p que --features loom lossless::loom_tests -- --test-threads=1
//! ```
//!
//! ```text
//! LOOM_MAX_PREEMPTIONS=3 LOOM_MAX_BRANCHES=10000 \
//!   cargo test -p que --features loom lossless::loom_tests -- --test-threads=1
//! ```

use std::mem::size_of;

use loom::thread;

use super::{consumer::Consumer, producer::Producer};
use crate::{Channel, ShmemMode};

macro_rules! channel_storage {
    ($name:ident, $n:literal) => {
        #[repr(C, align(128))]
        struct $name([u8; size_of::<Channel<ShmemMode, u64, $n>>()]);

        impl $name {
            fn new_zeroed() -> Self {
                Self([0; size_of::<Channel<ShmemMode, u64, $n>>()])
            }

            fn prepare(&mut self) -> *mut u8 {
                let ptr = self.0.as_mut_ptr();
                unsafe {
                    Channel::<ShmemMode, u64, $n>::loom_write_fresh_empty_at(
                        ptr,
                    );
                }
                ptr
            }
        }
    };
}

channel_storage!(ChannelStorage4, 4);
channel_storage!(ChannelStorage8, 8);
channel_storage!(ChannelStorage16, 16);

fn drain_all<const N: usize>(
    consumer: &mut Consumer<ShmemMode, u64, N>,
    expected_len: usize,
) -> Vec<u64> {
    let mut out = Vec::with_capacity(expected_len);
    while out.len() < expected_len {
        if let Some(v) = consumer.pop() {
            out.push(v);
        } else {
            thread::yield_now();
        }
    }
    out
}

fn push_retry<const N: usize>(
    producer: &mut Producer<ShmemMode, u64, N>,
    value: u64,
) {
    while producer.push(value).is_err() {
        thread::yield_now();
    }
}

#[test]
fn loom_lossless_batched_pushes() {
    loom::model(|| {
        let mut storage = ChannelStorage16::new_zeroed();
        let ptr = storage.prepare();

        let mut producer =
            unsafe { Producer::<ShmemMode, u64, 16>::join(ptr).unwrap() };
        let mut consumer =
            unsafe { Consumer::<ShmemMode, u64, 16>::join(ptr).unwrap() };

        let p = thread::spawn(move || {
            for i in 0u64..5 {
                push_retry(&mut producer, i);
            }
            producer.sync();
            for i in 5u64..10 {
                push_retry(&mut producer, i);
            }
            producer.sync();
        });

        let c = thread::spawn(move || {
            let got = drain_all(&mut consumer, 10);
            assert_eq!(got, (0u64..10).collect::<Vec<_>>());
        });

        p.join().unwrap();
        c.join().unwrap();
    });
}

#[test]
fn loom_lossless_fill_drain_refill() {
    loom::model(|| {
        let mut storage = ChannelStorage4::new_zeroed();
        let ptr = storage.prepare();

        let mut producer =
            unsafe { Producer::<ShmemMode, u64, 4>::join(ptr).unwrap() };
        let mut consumer =
            unsafe { Consumer::<ShmemMode, u64, 4>::join(ptr).unwrap() };

        let p = thread::spawn(move || {
            for round in 0u64..2 {
                let base = round * 3;
                for i in 0..3 {
                    push_retry(&mut producer, base + i);
                }
                producer.sync();
            }
        });

        let c = thread::spawn(move || {
            let first = drain_all(&mut consumer, 3);
            assert_eq!(first, vec![0, 1, 2]);
            let second = drain_all(&mut consumer, 3);
            assert_eq!(second, vec![3, 4, 5]);
        });

        p.join().unwrap();
        c.join().unwrap();
    });
}

#[test]
fn loom_lossless_no_false_some_on_empty() {
    loom::model(|| {
        let mut storage = ChannelStorage8::new_zeroed();
        let ptr = storage.prepare();

        let _producer =
            unsafe { Producer::<ShmemMode, u64, 8>::join(ptr).unwrap() };
        let mut consumer =
            unsafe { Consumer::<ShmemMode, u64, 8>::join(ptr).unwrap() };

        for _ in 0..8 {
            assert_eq!(consumer.pop(), None);
        }
    });
}
