use bytemuck::{Pod, Zeroable};
use criterion::{
    black_box, criterion_group, criterion_main, Criterion, Throughput,
};
use que::{
    headless_spmc::{consumer::Consumer, producer::Producer},
    shmem::cleanup_shmem,
    Channel,
};

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
        .mem_size(core::mem::size_of::<Channel<Transaction<1232>, N>>())
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
        Producer::<Transaction<1232>, N>::join_or_create_shmem(
            "sh_bench",
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };
    let mut consumer = unsafe {
        Consumer::<Transaction<1232>, N>::join_shmem(
            "sh_bench",
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };

    producer.push(black_box(&tx));
    producer.sync();
    assert_eq!(consumer.pop().unwrap(), tx);

    g.bench_function(
        "push_pop",
        #[inline(always)]
        |b| {
            b.iter(
                #[inline(always)]
                || {
                    producer.push(&tx);
                    producer.sync();
                    consumer.pop();
                },
            );
        },
    );
}

criterion_group!(spsc, push_pop);
criterion_main!(spsc);
