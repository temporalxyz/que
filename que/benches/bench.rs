use bytemuck::{Pod, Zeroable};
use criterion::{
    black_box, criterion_group, criterion_main, Criterion, Throughput,
};
use que::{
    headless_spmc::{consumer::Consumer, producer::Producer},
    shmem::cleanup_shmem,
    Channel, ShmemMode,
};
use std::sync::mpsc; // <-- add this

use que::page_size::PageSize;

#[derive(Copy, Clone, Zeroable, PartialEq, Debug)]
pub struct Transaction<const N: usize> {
    pub bytes: [u8; N],
}

unsafe impl Pod for Transaction<1232> {}

fn push_pop(c: &mut Criterion) {
    let mut g = c.benchmark_group("PushPop");
    g.throughput(Throughput::Bytes(1232));

    let tx = Transaction { bytes: [1; 1232] };
    const N: usize = 16384;

    #[cfg(target_os = "linux")]
    let page_size = PageSize::Huge;
    #[cfg(not(target_os = "linux"))]
    let page_size = PageSize::Standard;
    let buffer_size: i64 = page_size
        .mem_size(core::mem::size_of::<
            Channel<ShmemMode, Transaction<1232>, N>,
        >())
        .try_into()
        .unwrap();

    cleanup_shmem(
        "sh_bench",
        buffer_size,
        #[cfg(target_os = "linux")]
        page_size,
    )
    .ok();
    let mut producer = unsafe {
        Producer::<ShmemMode, Transaction<1232>, N>::join_or_create_shmem(
            "sh_bench",
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };
    let mut consumer = unsafe {
        Consumer::<ShmemMode, Transaction<1232>, N>::join_shmem(
            "sh_bench",
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };

    producer.push(black_box(tx));
    producer.sync();
    assert_eq!(consumer.pop().unwrap(), tx);

    g.bench_function(
        "push_pop",
        #[inline(always)]
        |b| {
            b.iter(
                #[inline(always)]
                || {
                    producer.push(tx);
                    producer.sync();
                    consumer.pop();
                },
            );
        },
    );
}

/// Also benchmark std::sync::mpsc::sync_channel
fn push_pop_std_mpsc_sync(c: &mut Criterion) {
    let mut g = c.benchmark_group("StdSyncMpsc");
    g.throughput(Throughput::Bytes(1232));

    let payload = Transaction { bytes: [1; 1232] };

    // Capacity 1 so each iteration is a strict send->recv (no buffering advantage).
    let (sender, receiver) = mpsc::sync_channel::<Transaction<1232>>(1);

    // Sanity check
    sender.send(payload).unwrap();
    assert_eq!(receiver.recv().unwrap(), payload);

    g.bench_function(
        "send_recv",
        #[inline(always)]
        |b| {
            b.iter(
                #[inline(always)]
                || {
                    // send a copy, then receive it back
                    sender.send(black_box(payload)).unwrap();
                    black_box(receiver.recv().unwrap());
                },
            );
        },
    );
}

criterion_group!(spsc, push_pop, push_pop_std_mpsc_sync);
criterion_main!(spsc);
