use que::{
    lossless::producer::Producer, page_size::PageSize,
    shmem::cleanup_shmem, Channel, ShmemMode,
};

const N: usize = 4;
type Element = u64;
const SPSC_SIZE: usize =
    core::mem::size_of::<Channel<ShmemMode, Element, N>>();

fn main() {
    let shmem_id = "shmem";

    // Initialize or join as producer
    let page_size = PageSize::Standard;
    eprintln!(
        "opening shmem of size {} with page size {:?}",
        SPSC_SIZE, page_size
    );

    let mut producer = unsafe {
        Producer::<ShmemMode, Element, N>::join_or_create_shmem(
            shmem_id,
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };
    eprintln!("initialized producer");

    // Wait for consumer to ack join
    eprintln!("waiting for consumer ack 1");
    while !producer.consumer_heartbeat() {}

    // Push 4 values
    eprintln!("pushing 4 values");
    let value = 42u64;
    for _ in 0..4 {
        producer.push(value).unwrap();
    }
    eprintln!("pushed 4 values {}", value);

    // Lossless push should fail
    assert!(producer.push(value).is_err());

    // Sync the producer
    producer.sync();
    eprintln!("published value");

    // Wait for consumer to ack message
    eprintln!("waiting for consumer ack 2");
    while !producer.consumer_heartbeat() {}
    eprintln!("sending ack 1");
    producer.beat();

    // Now via reserve/commit semantics
    eprintln!("reserving");
    let mut reservation = loop {
        if let Ok(res) = producer.reserve(1) {
            break res;
        }
    };

    reservation.write_next(420);
    reservation.commit();
    println!("reserve/commit published 420");

    // Wait for consumer to ack message
    eprintln!("waiting for consumer ack 3");
    while !producer.consumer_heartbeat() {}

    // Clean up
    let buffer_size: i64 = page_size
        .mem_size(SPSC_SIZE)
        .try_into()
        .unwrap();
    cleanup_shmem(
        shmem_id,
        buffer_size,
        #[cfg(target_os = "linux")]
        page_size,
    )
    .unwrap();

    eprintln!("done\n");
}
