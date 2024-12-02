use que::{
    headless_spmc::producer::Producer, page_size::PageSize,
    shmem::cleanup_shmem, Channel,
};

const N: usize = 4;
type Element = u64;
const SPSC_SIZE: usize = core::mem::size_of::<Channel<Element, N>>();

fn main() {
    // Initialize or join as producer
    let page_size = PageSize::Standard;
    let mut producer = unsafe {
        Producer::<Element, N>::join_or_create_shmem(
            "shmem",
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };

    // Wait for consumer
    println!("waiting for consumer");
    while !producer.consumer_heartbeat() {}

    // Push and publish
    let value = 42;
    producer.push(&value);
    producer.sync();
    println!("published value");

    // Wait for ack (using heartbeat as ack)
    println!("waiting for consumer ack");
    while !producer.consumer_heartbeat() {}
    println!("consumer ack'd");

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

    println!("done");
}
