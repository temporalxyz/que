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

    #[cfg(target_os = "linux")]
    let page_size = PageSize::Huge;
    #[cfg(not(target_os = "linux"))]
    let page_size = PageSize::Standard;

    let res = cleanup_shmem(
        "sh_bench",
        page_size.mem_size(TX_TEST_SIZE * N) as i64,
        #[cfg(target_os = "linux")]
        PageSize::Huge,
    );
    println!("{:?}", res);
    let mut producer = unsafe {
        Producer::<Transaction<TX_TEST_SIZE>, N>::join_or_create_shmem(
            "sh_bench",
            #[cfg(target_os = "linux")]
            PageSize::Huge,
        )
        .unwrap()
    };
    let mut consumer = unsafe {
        Consumer::<Transaction<TX_TEST_SIZE>, N>::join_shmem(
            "sh_bench",
            #[cfg(target_os = "linux")]
            PageSize::Huge,
        )
        .unwrap()
    };

    std::thread::Builder::new()
        .name("producer".to_string())
        .spawn(move || {
            let tid = get_thread_id();
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
            let tid = get_thread_id();
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
        #[cfg(target_os = "linux")]
        page_size,
    );
    println!("{:?}", res);
}

#[cfg(target_os = "linux")]
fn get_thread_id() -> nix::libc::pid_t {
    unsafe {
        nix::libc::syscall(nix::libc::SYS_gettid) as nix::libc::pid_t
    }
}

#[link(name = "pthread")]
extern "C" {
    fn pthread_threadid_np(
        pthread: *mut nix::libc::pthread_t,
        thread_id: *mut u64,
    ) -> nix::libc::c_int;
}
#[cfg(target_os = "macos")]
fn get_thread_id() -> u64 {
    let mut tid: u64 = 0;
    let ret = unsafe {
        pthread_threadid_np(std::ptr::null_mut(), &mut tid as *mut u64)
    };
    if ret != 0 {
        panic!("Failed to get thread ID from pthread_threadid_np. Error code: {}", ret);
    }
    tid
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
