# Que: A high performance bounded ipc spsc channel

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

This library provides an implementation for a high performance lock-free single-producer single-consumer[^1] channel which is bounded in capacity. It supports interthread and interprocess communication (ITC/IPC); it can be backed by process-private memory and shared between threads, or it can be backed by shared memory and be used for independent processes to pass messages. On Linux, the channel can be backed by huge pages for maximum TLB efficiency.


## Performance
With 1264 byte messages on an AMD EPYC 9254 24-Core Processor, we measure over 50M messages/second produced and consumed (over 500 Gbps). This is more than an order of magnitude higher than what can be obtained from using `std::sync::mpsc` as an spsc channel (≈3M messages/second).

## Setup

#### Process-private
Nothing special is needed if using process-private memory. You can allocate a buffer via something like

```rust
/// This leaks! Remember to deallocate!
fn new_spsc_buffer<T: AnyBitPattern, const N: usize>() -> *mut u8 {
    let buffer_size = size_of::<que::spsc::SPSC<Element, N>>();
    let layout = Layout::from_size_align(buffer_size, 128).unwrap();

    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        panic!("alloc failed")
    }
    ptr
}
```

and then use this buffer to initialize the spsc and join as a producer/consumer via something like

```rust
const N: usize = 1024;
type Element = u64;

// Allocate buffer (process-exclusive memory)
let buffer = new_spsc_buffer::<Element, N>();

// Create producer & consumer
let mut producer = unsafe {
    Producer::<Element, N>::initialize_in(buffer).unwrap()
};
let mut consumer =
    unsafe { Consumer::<Element, N>::join(buffer).unwrap() };
```

See `examples/spsc.rs` for this example in action.

#### Shared memory

To use a `shmem` object, simply use the provided functions

```rust
let use_huge_pages = false;

// Create producer & consumer
let mut producer = unsafe {
    Producer::<Element, N>::join_or_create_shmem(
        "shmem",
        #[cfg(target_os = "linux")]
        use_huge_pages,
    )
    .unwrap()
};
let mut consumer = unsafe {
    Consumer::<Element, N>::join_shmem(
        "shmem",
        #[cfg(target_os = "linux")]
        use_huge_pages,
    )
    .unwrap()
};
```

##### Huge Pages
To make use of huge pages on Linux, you must first mount hugepages using `./mount_huge_and_gigantic.sh` and then allocate some number of huge pages via `./hp.sh <N>`. By default, this uses 2MB pages so e.g. to preallocate 32MB use `./hp.sh 16`.


##### Headless & Lossless mode
There is a headless SPMC and a lossless SPSC. 

###### Headless

This is a fast but lossy channel. The producer will overwrite old elements when the buffer is full. This is done to achieve maximum performance, as an `is_full` check does not need to be repeated on each push, but it is not suitable for the general case. Multiple consumers can read values without any runtime coordination by reading values with arbitrary stride (e.g. two consumers can read `[0, 2, 4, ..]` and `[1, 3, 5, ..]`).

###### Lossless

The lossless channel is an sps cwhich restores the atomic head index and prevents the producer from writing when the buffer is full, in addition to restoring FIFO ordering.

#####


[^1]: There is a multi-consumer mode for the headless spsc, but it is not FIFO!

## Legal & Disclaimer

⚠️ **Important**: This code is provided "as is" without warranty of any kind. It has not been audited and may contain bugs or security vulnerabilities. See [DISCLAIMER.md](./DISCLAIMER.md) for important usage notes and warnings.

This project is licensed under the Apache License, Version 2.0. See [LICENSE](./LICENSE) for the full license text.

