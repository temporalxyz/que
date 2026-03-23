use que::{lossless::consumer::Consumer, ShmemMode};

#[cfg(target_os = "linux")]
use que::page_size::PageSize;

const N: usize = 4;
type Element = u64;

fn main() {
    let shmem_id = "shmem";

    // Open shared memory
    #[cfg(target_os = "linux")]
    let page_size = PageSize::Standard;
    const SPSC_SIZE: usize =
        core::mem::size_of::<que::Channel<ShmemMode, Element, N>>();

    eprintln!(
        "opening shmem of size {} with page size {:?}",
        SPSC_SIZE, page_size
    );

    // Join as consumer (must be initialized already)
    eprintln!("joining consumer");
    let mut consumer = unsafe {
        Consumer::<ShmemMode, Element, N>::join_shmem(
            shmem_id,
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };
    eprintln!("joined consumer");

    // Ack join
    eprintln!("sent consumer ack 1");
    consumer.beat();

    // Read 4 values
    for _ in 0..4 {
        loop {
            if let Some(value) = consumer.pop() {
                eprintln!("read value {}", value);
                break;
            }
            // wait for producer to publish
        }
    }

    // Ack message
    consumer.beat();
    eprintln!("sent consumer ack 2");

    eprintln!("waiting for producer ack");
    while !consumer.producer_heartbeat() {}

    // Read value
    eprintln!("reading value");
    let value = loop {
        if let Some(value) = consumer.pop() {
            break value;
        }
        // wait for producer to publish
    };
    eprintln!("read value {}", value);

    // Ack message
    consumer.beat();
    eprintln!("sent consumer ack 3");

    eprintln!("done\n");
}
