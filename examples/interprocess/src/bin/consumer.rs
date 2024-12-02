use que::{headless_spmc::consumer::Consumer, page_size::PageSize};

const N: usize = 4;
type Element = u64;

fn main() {
    // This will panic if producer does not initialize/join first!
    let page_size = PageSize::Standard;
    const SPSC_SIZE: usize =
        core::mem::size_of::<que::Channel<Element, N>>();
    println!("spsc has size {SPSC_SIZE}");
    println!(
        "Size of SPSC struct without buffer: {}",
        std::mem::size_of::<que::Channel<(), 0>>()
    );

    let mut consumer = unsafe {
        Consumer::<Element, N>::join_shmem(
            "shmem",
            #[cfg(target_os = "linux")]
            page_size,
        )
        .unwrap()
    };

    // Use heartbeat as join ack
    consumer.beat();

    loop {
        if let Some(value) = consumer.pop() {
            println!("received value {value}");

            // Use heartbeat as receive ack
            consumer.beat();
            break;
        }
    }
}
