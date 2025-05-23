use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use bytemuck::AnyBitPattern;

use crate::{
    error::QueError, page_size::PageSize, shmem::Shmem, MAGIC,
};

use super::{burst_amount, Channel};

#[repr(C, align(128))]
pub struct Producer<T, const N: usize> {
    spsc: NonNull<Channel<T, N>>,
    tail: usize,
    /// Number of elements written since last sync
    written: usize,
    last_consumer_heartbeat: usize,
}

unsafe impl<T, const N: usize> Send for Producer<T, N> {}

impl<T: AnyBitPattern, const N: usize> Producer<T, N> {
    /// Joins or creates a channel backed by shared memory as a
    /// producer.
    pub unsafe fn join_or_create_shmem(
        shmem_id: &str,
        #[cfg(target_os = "linux")] page_size: PageSize,
    ) -> Result<Producer<T, N>, QueError> {
        #[cfg(not(target_os = "linux"))]
        let page_size = PageSize::Standard;

        // Calculate buffer size.
        // If using huge pages, we must uplign to page size.
        let buffer_size: i64 = page_size
            .mem_size(core::mem::size_of::<Channel<T, N>>())
            .try_into()
            .map_err(|_| QueError::InvalidSize)?;

        // Open or create shmem
        let shmem = Shmem::open_or_create(
            shmem_id,
            buffer_size,
            #[cfg(target_os = "linux")]
            page_size,
        )?;

        // Zerocopy deserialize the SPSC
        let spsc: &Channel<T, N> =
            unsafe { &*shmem.get_mut_ptr().cast() };

        // Check magic
        let magic = spsc.magic.load(Ordering::Acquire);
        let capacity = spsc.capacity.load(Ordering::Acquire);
        if magic == MAGIC {
            // Check capacity
            if capacity != N {
                return Err(QueError::IncorrectCapacity(capacity));
            }

            // Successful join if magic and capacity is correct
            Ok(Producer {
                spsc: NonNull::new(shmem.get_mut_ptr().cast()).unwrap(),
                tail: spsc.tail.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: spsc
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else if magic == 0 {
            // Initialize
            let Channel {
                tail,
                // consumer will set this
                head: _,
                capacity,
                producer_heartbeat,
                consumer_heartbeat: _,
                magic,
                buffer: _unused,
                padding: _,
            } = spsc;

            tail.store(0, Ordering::Release);
            producer_heartbeat.store(0, Ordering::Release);
            capacity.store(N, Ordering::Release);
            magic.store(MAGIC, Ordering::Release);

            Ok(Producer {
                spsc: NonNull::new(shmem.get_mut_ptr().cast()).unwrap(),
                tail: 0,
                written: 0,
                last_consumer_heartbeat: spsc
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else {
            // Magic is not MAGIC and not zero
            Err(QueError::CorruptionDetected)
        }
    }

    /// Initializes a channel backed by `buffer` and joins as a
    /// producer.
    ///
    /// SAFETY:
    /// This must point to a buffer of proper size and alignment.
    pub unsafe fn initialize_in(
        buffer: *mut u8,
    ) -> Result<Producer<T, N>, QueError> {
        assert!(
            N > 0 && N.is_power_of_two(),
            "Capacity must be a power of two"
        );
        assert!(buffer as usize % 128 == 0, "unaligned");

        // Zerocopy deserialize the SPSC
        let spsc: &Channel<T, N> = &*buffer.cast();

        // Check magic
        let magic = spsc.magic.load(Ordering::Acquire);
        let capacity = spsc.capacity.load(Ordering::Acquire);
        if magic == MAGIC {
            // Check capacity
            if capacity != N {
                return Err(QueError::IncorrectCapacity(capacity));
            }

            spsc.producer_heartbeat
                .fetch_add(1, Ordering::Release);

            // Successful join if magic and capacity is correct
            Ok(Producer {
                spsc: NonNull::new(buffer.cast()).unwrap(),
                tail: spsc.tail.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: spsc
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else if magic == 0 {
            // Initialize
            let Channel {
                tail,
                // consumer will set this
                head: _,
                capacity,
                producer_heartbeat,
                consumer_heartbeat: _,
                magic,
                buffer: _unused,
                padding: _,
            } = spsc;

            tail.store(0, Ordering::Release);
            producer_heartbeat.store(0, Ordering::Release);
            capacity.store(N, Ordering::Release);
            magic.store(MAGIC, Ordering::Release);

            Ok(Producer {
                spsc: NonNull::new(buffer.cast()).unwrap(),
                tail: 0,
                written: 0,
                last_consumer_heartbeat: spsc
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else {
            // Magic is not MAGIC and not zero
            Err(QueError::CorruptionDetected)
        }
    }

    /// Joins an existing channel backed by `buffer` as a producer.
    ///
    /// SAFETY:
    /// This must point to a buffer of proper size and alignment.
    pub unsafe fn join(
        buffer: *mut u8,
    ) -> Result<Producer<T, N>, QueError> {
        assert!(
            N > 0 && N.is_power_of_two(),
            "Capacity must be a power of two"
        );
        assert!(buffer as usize % 128 == 0, "unaligned");

        // Zerocopy deserialize the SPSC
        let spsc: &Channel<T, N> = &*buffer.cast();

        let magic = spsc.magic.load(Ordering::Acquire);
        let capacity = spsc.capacity.load(Ordering::Acquire);
        if magic == MAGIC {
            if capacity != N {
                return Err(QueError::IncorrectCapacity(capacity));
            }

            // Successful join if magic and capacity is correct
            Ok(Producer {
                spsc: NonNull::new(buffer.cast()).unwrap(),
                tail: spsc.tail.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: spsc
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else if magic == 0 {
            // Technically could be corrupted but uninitialized
            // is most likely explanation
            Err(QueError::Uninitialized)
        } else {
            // Magic is not MAGIC and not zero
            Err(QueError::CorruptionDetected)
        }
    }

    /// Attempts to write a new element to the channel. If full, returns
    /// [QueError::Full].
    #[inline(always)]
    pub fn push(&mut self, value: &T) -> Result<(), QueError> {
        // Check if full
        let is_full = self.tail
            == unsafe {
                (*self.spsc.as_ptr())
                    .head
                    .load(Ordering::Relaxed)
            } + N;
        if is_full {
            return Err(QueError::Full);
        }

        // Write value if not full
        let index = self.tail & (N - 1);
        unsafe {
            *(*self.spsc.as_ptr())
                .buffer
                .as_mut_ptr()
                .add(index) = *value;
        };

        // Increment tail and written counter
        self.tail += 1;
        self.written += 1;

        // Update tail if we've written past burst amount and haven't
        // updated shared atomic.
        if self.written == burst_amount::<N>() {
            self.sync();
        }

        Ok(())
    }

    /// Increments the producer heartbeat.
    ///
    /// Can be read by the consumer to see that the producer is still
    /// online if done periodically. Can also be used to ack individual
    /// messages or alert that we've joined.
    pub fn beat(&self) {
        unsafe {
            (*self.spsc.as_ptr())
                .producer_heartbeat
                .fetch_add(1, Ordering::Release);
        }
    }

    /// Synchronizes the local tail with the atomic tail in the channel,
    /// publishing newly written values.
    #[inline(always)]
    pub fn sync(&mut self) {
        self.written = 0;
        unsafe {
            (*self.spsc.as_ptr())
                .tail
                .store(self.tail, Ordering::Release)
        }
    }

    /// Checks if a consumer has incremented its heartbeat since last
    /// called. Can be used by the producer to see if the consumer is
    /// still online if done periodically. Can also be used to ack
    /// individual messages or alert that we've joined.
    pub fn consumer_heartbeat(&mut self) -> bool {
        let heartbeat = unsafe {
            (*self.spsc.as_ptr())
                .consumer_heartbeat
                .load(Ordering::Acquire)
        };

        if heartbeat != self.last_consumer_heartbeat {
            self.last_consumer_heartbeat = heartbeat;
            true
        } else {
            false
        }
    }

    /// Returns pointer to inner padding.
    ///
    /// User is responsible for safe usage.
    ///
    /// Can be used to store metadata (e.g. hash seed).
    ///
    /// Byte array is 128 byte aligned.
    pub fn get_padding_ptr(&self) -> NonNull<[u8; 112]> {
        unsafe {
            NonNull::new_unchecked(
                self.spsc.cast::<u8>().as_ptr().add(512),
            )
            .cast()
        }
    }
}
