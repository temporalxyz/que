use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use bytemuck::AnyBitPattern;

use crate::{
    error::QueError, headless_spmc::MAGIC, page_size::PageSize,
    shmem::Shmem,
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
        let spsc: &mut Channel<T, N> =
            unsafe { &mut *shmem.get_mut_ptr().cast() };

        // Check magic
        if spsc.magic == MAGIC {
            // Check capacity
            if spsc.capacity != N {
                return Err(QueError::IncorrectCapacity(spsc.capacity));
            }

            spsc.producer_heartbeat
                .fetch_add(1, Ordering::Release);

            // Successful join if magic and capacity is correct
            Ok(Producer {
                spsc: NonNull::new(spsc).unwrap(),
                tail: spsc.tail.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: spsc
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else if spsc.magic == 0 {
            // Initialize
            let Channel {
                tail,
                head: _, // not used in headless mode
                capacity,
                producer_heartbeat,
                consumer_heartbeat: _,
                magic,
                buffer: _unused,
            } = spsc;

            tail.store(0, Ordering::Release);
            producer_heartbeat.store(0, Ordering::Release);
            *capacity = N;
            *magic = MAGIC;

            Ok(Producer {
                spsc: NonNull::new(spsc).unwrap(),
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

    pub unsafe fn initialize_in(
        buffer: *mut u8,
    ) -> Result<Producer<T, N>, QueError> {
        assert!(
            N > 0 && N.is_power_of_two(),
            "Capacity must be a power of two"
        );
        assert!(buffer as usize % 128 == 0, "unaligned");

        // Zerocopy deserialize the SPSC
        let spsc: &mut Channel<T, N> = &mut *buffer.cast();

        // Check magic
        if spsc.magic == MAGIC {
            // Check capacity
            if spsc.capacity != N {
                return Err(QueError::IncorrectCapacity(spsc.capacity));
            }

            spsc.producer_heartbeat
                .fetch_add(1, Ordering::Release);

            // Successful join if magic and capacity is correct
            Ok(Producer {
                spsc: NonNull::new(spsc).unwrap(),
                tail: spsc.tail.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: spsc
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else if spsc.magic == 0 {
            // Initialize
            let Channel {
                tail,
                head: _, // not used in headless mode
                capacity,
                producer_heartbeat,
                consumer_heartbeat: _,
                magic,
                buffer: _unused,
            } = spsc;

            tail.store(0, Ordering::Release);
            producer_heartbeat.store(0, Ordering::Release);
            *capacity = N;
            *magic = MAGIC;

            Ok(Producer {
                spsc: NonNull::new(spsc).unwrap(),
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

    pub unsafe fn join(
        buffer: *mut u8,
    ) -> Result<Producer<T, N>, QueError> {
        assert!(
            N > 0 && N.is_power_of_two(),
            "Capacity must be a power of two"
        );
        assert!(buffer as usize % 128 == 0, "unaligned");

        // Zerocopy deserialize the SPSC
        let spsc: &mut Channel<T, N> = &mut *buffer.cast();

        if spsc.magic == MAGIC {
            if spsc.capacity != N {
                return Err(QueError::IncorrectCapacity(spsc.capacity));
            }

            // Successful join if magic and capacity is correct
            Ok(Producer {
                spsc: NonNull::new(spsc).unwrap(),
                tail: spsc.tail.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: spsc
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else if spsc.magic == 0 {
            // Technically could be corrupted but uninitialized
            // is most likely explanation
            Err(QueError::Uninitialized)
        } else {
            // Magic is not MAGIC and not zero
            Err(QueError::CorruptionDetected)
        }
    }

    #[inline(always)]
    pub fn sync(&mut self) {
        self.written = 0;
        unsafe {
            self.spsc
                .as_mut()
                .tail
                .store(self.tail, Ordering::Release)
        }
    }

    #[inline(always)]
    pub fn push(&mut self, value: &T) {
        let spsc = unsafe { self.spsc.as_mut() };

        // Update tail if we've written past burst amount and haven't
        // updated shared atomic.
        if self.written == burst_amount::<N>() {
            self.sync();
        }

        // Write value
        let index = self.tail & (N - 1);
        unsafe {
            *spsc.buffer.as_mut_ptr().add(index) = *value;
        };

        // Increment tail and written counter
        self.tail += 1;
        self.written += 1;
    }

    pub fn beat(&self) {
        unsafe {
            self.spsc
                .as_ref()
                .producer_heartbeat
                .fetch_add(1, Ordering::Release);
        }
    }

    pub fn consumer_heartbeat(&mut self) -> bool {
        let heartbeat = unsafe {
            self.spsc
                .as_ref()
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
}
