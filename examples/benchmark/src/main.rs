use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use bytemuck::{Pod, Zeroable};
use que::{
    headless_spmc::{consumer::Consumer, producer::Producer},
    page_size::PageSize,
    shmem::cleanup_shmem,
};

#[derive(Copy, Clone, Zeroable, PartialEq, Debug)]
#[repr(C, align(128))]
pub struct Transaction<const N: usize> {
    pub bytes: [u8; N],
}

unsafe impl Pod for Transaction<TX_TEST_SIZE> {}
const ITERS: usize = 100_000_000;
const TX_TEST_SIZE: usize = 1232;
const N: usize = 8192;
static START_FLAG: AtomicBool = AtomicBool::new(false);
fn main() {
    let tx = Transaction {
        bytes: [1; TX_TEST_SIZE],
    };

    let page_size = PageSize::Huge;

    let res = cleanup_shmem(
        "sh_bench",
        page_size.mem_size(TX_TEST_SIZE * N) as i64,
        PageSize::Huge,
    );
    println!("{:?}", res);
    let mut producer = unsafe {
        Producer::<Transaction<TX_TEST_SIZE>, N>::join_or_create_shmem(
            "sh_bench",
            PageSize::Huge,
        )
        .unwrap()
    };
    let mut consumer = unsafe {
        Consumer::<Transaction<TX_TEST_SIZE>, N>::join_shmem(
            "sh_bench",
            PageSize::Huge,
        )
        .unwrap()
    };

    std::thread::Builder::new()
        .name("producer".to_string())
        .spawn(move || {
            let tid = unsafe { get_thread_id() };
            println!("Producer thread TID: {:?}", tid);
            while !START_FLAG.load(std::sync::atomic::Ordering::Relaxed)
            {
            }
            let timer = Instant::now();
            for _ in 0..ITERS {
                producer.push(&tx);
                producer.sync();
            }
            let elapsed = timer.elapsed();
            println!(
                "{ITERS} in {:.3} s; {:.3} gbps",
                elapsed.as_secs_f32(),
                1232.0 * 8.0 * ITERS as f32
                    / elapsed.as_secs_f32()
                    / 1e9,
            );
        })
        .unwrap();

    let h2 = std::thread::Builder::new()
        .name("consumer".to_string())
        .spawn(move || {
            let tid = unsafe { get_thread_id() };
            println!("Consumer thread TID: {:?}", tid);
            while !START_FLAG.load(std::sync::atomic::Ordering::Relaxed)
            {
            }
            let timer = Instant::now();
            let empty = consume_until_empty(&mut consumer);
            let elapsed = timer.elapsed();

            println!(
                "{ITERS} in {:.3} s; {:.3} gbps empty {}",
                elapsed.as_secs_f32(),
                1232.0 * 8.0 * ITERS as f32
                    / elapsed.as_secs_f32()
                    / 1e9,
                empty
            );
        })
        .unwrap();

    println!("Starting consumer");

    //spin for 1 second to warm up
    let start = std::time::Instant::now();
    loop {
        let now = Instant::now();
        if now.duration_since(start) > Duration::from_secs(5) {
            break;
        }
    }
    println!("beginning bench");
    START_FLAG.store(true, std::sync::atomic::Ordering::Relaxed);
    h2.join().unwrap();

    let res = cleanup_shmem(
        "sh_bench",
        page_size.mem_size(TX_TEST_SIZE * N) as i64,
        page_size,
    );
    println!("{:?}", res);
}

unsafe fn get_thread_id() -> nix::libc::pid_t {
    nix::libc::syscall(nix::libc::SYS_gettid) as nix::libc::pid_t
}

#[inline(never)]
fn consume_until_empty(
    consumer: &mut Consumer<Transaction<TX_TEST_SIZE>, N>,
) -> usize {
    let mut consumed: usize = 0;
    let mut empty: usize = 0;
    while consumed < ITERS {
        let pop_res_usize = consumer.pop().is_some() as usize;
        consumed += pop_res_usize;
        empty += pop_res_usize ^ (true as usize)
    }
    empty
}
