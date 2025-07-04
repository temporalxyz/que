use std::mem::size_of;

use que::{
    headless_spmc::{
        consumer::Consumer, headless_pair, producer::Producer,
    },
    page_size::PageSize,
    shmem::cleanup_shmem,
    Channel, LocalMode, ShmemMode,
};

const N: usize = 1024;
type Element = u64;
const SPSC_SIZE: usize = size_of::<Channel<LocalMode, Element, N>>();

fn main() {
    with_process_exclusive_mem();
    with_shmem(PageSize::Standard);
    #[cfg(target_os = "linux")]
    with_shmem(PageSize::Huge);
}

fn with_process_exclusive_mem() {
    // Create producer & consumer
    let (mut producer, mut consumer) = headless_pair::<Element, N>();

    // Push & pop
    producer.push(69);
    producer.sync();
    assert_eq!(consumer.pop().unwrap(), 69);

    println!("with process-exclusive memory complete\n");
}

fn with_shmem(page_size: PageSize) {
    // Create producer & consumer
    let mut producer = unsafe {
        Producer::<ShmemMode, Element, N>::join_or_create_shmem(
            "shmem",
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };
    let mut consumer = unsafe {
        Consumer::<ShmemMode, Element, N>::join_shmem(
            "shmem",
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };

    // Push & pop
    producer.push(69);
    producer.sync();
    assert_eq!(consumer.pop().unwrap(), 69);

    // Cleanup
    let buffer_size: i64 = page_size
        .mem_size(SPSC_SIZE)
        .try_into()
        .unwrap();
    cleanup_shmem(
        "shmem",
        buffer_size,
        #[cfg(target_os = "linux")]
        page_size,
    )
    .unwrap();

    match page_size {
        PageSize::Standard => println!("with shared memory complete\n"),
        #[cfg(target_os = "linux")]
        PageSize::Huge => println!("with huge pages complete\n"),
        #[cfg(target_os = "linux")]
        PageSize::Gigantic => {
            println!("with gigantic pages complete\n")
        }
    }
}
