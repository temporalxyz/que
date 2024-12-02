use std::{alloc::Layout, mem::size_of};

use bytemuck::AnyBitPattern;
use que::{
    headless_spmc::{consumer::Consumer, producer::Producer},
    page_size::PageSize,
    shmem::cleanup_shmem,
    Channel,
};

/// This leaks! Only for tests!
fn new_spsc_buffer<T: AnyBitPattern, const N: usize>() -> *mut u8 {
    let buffer_size = size_of::<Channel<Element, N>>();
    let layout = Layout::from_size_align(buffer_size, 128).unwrap();

    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        panic!("alloc failed")
    }
    ptr
}

const N: usize = 1024;
type Element = u64;
const SPSC_SIZE: usize = size_of::<Channel<Element, N>>();

fn main() {
    with_process_exclusive_mem();
    with_shmem(PageSize::Standard);
    #[cfg(target_os = "linux")]
    with_shmem(PageSize::Huge);
}

fn with_process_exclusive_mem() {
    // Allocate buffer (process-exclusive memory)
    let buffer = new_spsc_buffer::<Element, N>();

    // Create producer & consumer
    let mut producer = unsafe {
        Producer::<Element, N>::initialize_in(buffer).unwrap()
    };
    let mut consumer =
        unsafe { Consumer::<Element, N>::join(buffer).unwrap() };

    // Push & pop
    producer.push(&69);
    producer.sync();
    assert_eq!(consumer.pop().unwrap(), 69);

    println!("with process-exclusive memory complete\n");
}

fn with_shmem(page_size: PageSize) {
    // Create producer & consumer
    let mut producer = unsafe {
        Producer::<Element, N>::join_or_create_shmem(
            "shmem",
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };
    let mut consumer = unsafe {
        Consumer::<Element, N>::join_shmem(
            "shmem",
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };

    // Push & pop
    producer.push(&69);
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
