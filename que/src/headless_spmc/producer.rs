use std::sync::atomic::Ordering;
use std::{ptr::NonNull, sync::Arc};

use bytemuck::AnyBitPattern;

use crate::{
    error::QueError, headless_spmc::MAGIC, page_size::PageSize,
    shmem::Shmem, ChannelMode, ShmemMode,
};

use super::{burst_amount, Channel};

#[repr(C, align(128))]
pub struct Producer<M: ChannelMode<T>, T, const N: usize> {
    spsc: NonNull<Channel<M, T, N>>,
    tail: usize,
    /// Number of elements written since last sync
    written: usize,
    last_consumer_heartbeat: usize,
}

unsafe impl<M: ChannelMode<T>, T, const N: usize> Send
    for Producer<M, T, N>
{
}

impl<T: AnyBitPattern, const N: usize> Producer<ShmemMode, T, N> {
    /// Joins or creates a channel backed by shared memory as a
    /// producer.
    pub unsafe fn join_or_create_shmem(
        shmem_id: &str,
        #[cfg(target_os = "linux")] page_size: PageSize,
    ) -> Result<Producer<ShmemMode, T, N>, QueError> {
        #[cfg(not(target_os = "linux"))]
        let page_size = PageSize::Standard;

        // Calculate buffer size.
        // If using huge pages, we must uplign to page size.
        let buffer_size: i64 = page_size
            .mem_size(core::mem::size_of::<Channel<ShmemMode, T, N>>())
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
        let spsc: *mut Channel<ShmemMode, T, N> =
            shmem.get_mut_ptr().cast();

        // Check magic
        let magic = (*spsc).magic.load(Ordering::Acquire);
        let capacity = (*spsc).capacity.load(Ordering::Acquire);
        #[rustfmt::skip]
        return if magic == MAGIC {
            // Check capacity
            if capacity != N {
                return Err(QueError::IncorrectCapacity(capacity));
            }

            (*spsc)
                .producer_heartbeat
                .fetch_add(1, Ordering::Release);

            // Successful join if magic and capacity is correct
            Ok(Producer {
                spsc: NonNull::new(shmem.get_mut_ptr().cast()).unwrap(),
                tail: (*spsc).tail.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: (*spsc).consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else if magic == 0 {
            (*spsc).tail.store(0, Ordering::Release);
            (*spsc).producer_heartbeat.store(0, Ordering::Release);
            (*spsc).consumer_heartbeat.store(0, Ordering::Release);
            (*spsc).capacity.store(N, Ordering::Release);
            (*spsc).magic.store(MAGIC, Ordering::Release);

            Ok(Producer {
                spsc: NonNull::new(shmem.get_mut_ptr().cast()).unwrap(),
                tail: 0,
                written: 0,
                last_consumer_heartbeat: (*spsc)
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else {
            // Magic is not MAGIC and not zero
            Err(QueError::CorruptionDetected)
        };
    }

    /// Initializes a channel backed by `buffer` and joins as a
    /// producer.
    ///
    /// SAFETY:
    /// This must point to a buffer of proper size and alignment.
    ///
    /// In LocalMode, must point to a region allocated by an Arc with the strong count not yet incremented!
    pub unsafe fn join_or_initialize_in(
        buffer: *mut u8,
    ) -> Result<Producer<ShmemMode, T, N>, QueError> {
        Self::join_or_initialize_in_(buffer)
    }

    /// Joins an existing channel backed by `buffer` as a producer.
    ///
    /// SAFETY:
    /// This must point to a buffer of proper size and alignment.
    ///
    /// In LocalMode, must point to a region allocated by an Arc with the strong count not yet incremented!
    pub unsafe fn join(
        buffer: *mut u8,
    ) -> Result<Producer<ShmemMode, T, N>, QueError> {
        Self::join_(buffer)
    }
}

impl<M: ChannelMode<T>, T, const N: usize> Producer<M, T, N> {
    pub const MODULO_MASK: usize = N - 1;
    pub(crate) unsafe fn join_(
        buffer: *mut u8,
    ) -> Result<Producer<M, T, N>, QueError> {
        assert!(
            N > 0 && N.is_power_of_two(),
            "Capacity must be a power of two"
        );
        assert!(buffer as usize % 128 == 0, "unaligned");

        // Zerocopy deserialize the SPSC
        let spsc: *mut Channel<M, T, N> = buffer.cast();

        let magic = (*spsc).magic.load(Ordering::Acquire);
        let capacity = (*spsc).capacity.load(Ordering::Acquire);
        if magic == MAGIC {
            if capacity != N {
                return Err(QueError::IncorrectCapacity(capacity));
            }

            if M::BACKED_BY_ARCC {
                unsafe {
                    Arc::increment_strong_count(spsc);
                }
            }

            // Successful join if magic and capacity is correct
            Ok(Producer {
                spsc: NonNull::new(buffer.cast()).unwrap(),
                tail: (*spsc).tail.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: (*spsc)
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

    pub(crate) unsafe fn join_or_initialize_in_(
        buffer: *mut u8,
    ) -> Result<Producer<M, T, N>, QueError> {
        assert!(
            N > 0 && N.is_power_of_two(),
            "Capacity must be a power of two"
        );
        assert!(buffer as usize % 128 == 0, "unaligned");

        let spsc: *mut Channel<M, T, N> = buffer.cast();

        // Check magic
        let magic = (*spsc).magic.load(Ordering::Acquire);
        let capacity = (*spsc).capacity.load(Ordering::Acquire);
        #[rustfmt::skip]
        return if magic == MAGIC {
            // Check capacity
            if capacity != N {
                return Err(QueError::IncorrectCapacity(capacity));
            }

            (*spsc)
                .producer_heartbeat
                .fetch_add(1, Ordering::Release);

                if M::BACKED_BY_ARCC {
                    unsafe {
                        Arc::increment_strong_count(spsc);
                    }
                }


            // Successful join if magic and capacity is correct
            Ok(Producer {
                spsc: NonNull::new(buffer.cast()).unwrap(),
                tail: (*spsc).tail.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: (*spsc)
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else if magic == 0 {
            // When we initialize we must write this before a consumer joins
            // (for a consumer to join the capacity/magic must be written)
            (*spsc).magic.store(0, Ordering::Release);
            (*spsc).tail.store(0, Ordering::Release);
            (*spsc).producer_heartbeat.store(0, Ordering::Release);
            (*spsc).consumer_heartbeat.store(0, Ordering::Release);
            (*spsc).capacity.store(N, Ordering::Release);
            (*spsc).magic.store(MAGIC, Ordering::Release);

            if M::BACKED_BY_ARCC {
                unsafe {
                    Arc::increment_strong_count(spsc);
                }
            }

            Ok(Producer {
                spsc: NonNull::new(buffer.cast()).unwrap(),
                tail: 0,
                written: 0,
                last_consumer_heartbeat: (*spsc)
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else {
            // Magic is not MAGIC and not zero
            Err(QueError::CorruptionDetected)
        };
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

    /// Write a new element to the channel.
    #[inline(always)]
    pub fn push(&mut self, value: T) {
        // Update tail if we've written past burst amount and haven't
        // updated shared atomic.
        if self.written == burst_amount::<N>() {
            self.sync();
        }

        // Write value
        let index = self.tail & Self::MODULO_MASK;
        unsafe {
            *(*self.spsc.as_ptr())
                .buffer
                .as_mut_ptr()
                .add(index) = value;
        };

        // Increment tail and written counter
        self.tail += 1;
        self.written += 1;
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

impl<M: ChannelMode<T>, T, const N: usize> Drop for Producer<M, T, N> {
    fn drop(&mut self) {
        // LocalMode is backed by arc
        if M::BACKED_BY_ARCC {
            unsafe { drop(Arc::from_raw(self.spsc.as_ptr())) }
        }
    }
}
