use std::{ptr::NonNull, sync::Arc};

use bytemuck::AnyBitPattern;

use crate::{
    atomic_compat::Ordering,
    error::QueError, page_size::PageSize, shmem::Shmem, ChannelMode,
    ShmemMode, MAGIC,
};

use super::{burst_amount, Channel};

#[repr(C, align(128))]
pub struct Producer<M: ChannelMode<T>, T, const N: usize> {
    spsc: NonNull<Channel<M, T, N>>,
    tail: usize,
    head_cache: usize,
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

            // Successful join if magic and capacity is correct
            Ok(Producer {
                spsc: NonNull::new(shmem.get_mut_ptr().cast()).unwrap(),
                tail: (*spsc).tail.load(Ordering::Acquire),
                head_cache: (*spsc).head.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: (*spsc)
                    .consumer_heartbeat
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
                head_cache: 0,
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
    pub unsafe fn join_or_initialize_in(
        buffer: *mut u8,
    ) -> Result<Producer<ShmemMode, T, N>, QueError> {
        Self::join_or_initialize_in_(buffer)
    }

    /// Joins an existing channel backed by `buffer` as a producer.
    ///
    /// SAFETY:
    /// This must point to a buffer of proper size and alignment.
    pub unsafe fn join(
        buffer: *mut u8,
    ) -> Result<Producer<ShmemMode, T, N>, QueError> {
        Self::join_(buffer)
    }
}

impl<M: ChannelMode<T>, T, const N: usize> Producer<M, T, N> {
    pub const MODULO_MASK: usize = N - 1;

    pub(crate) unsafe fn join_or_initialize_in_(
        buffer: *mut u8,
    ) -> Result<Producer<M, T, N>, QueError> {
        assert!(
            N > 0 && N.is_power_of_two(),
            "Capacity must be a power of two"
        );
        assert!(buffer as usize % 128 == 0, "unaligned");

        // Zerocopy deserialize the SPSC
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
                head_cache: (*spsc).head.load(Ordering::Acquire),
                written: 0,
                last_consumer_heartbeat: (*spsc)
                    .consumer_heartbeat
                    .load(Ordering::Acquire),
            })
        } else if magic == 0 {
            (*spsc).tail.store(0, Ordering::Release);
            (*spsc).consumer_heartbeat.store(0, Ordering::Release);
            (*spsc).producer_heartbeat.store(0, Ordering::Release);
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
                head_cache: 0,
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
                head_cache: (*spsc).head.load(Ordering::Acquire),
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

    /// Attempts to write a new element to the channel. If full, returns
    /// [QueError::Full].
    #[inline(always)]
    pub fn push(&mut self, value: T) -> Result<(), QueError> {
        // Check if full
        if self.tail == self.head_cache + N {
            self.head_cache = unsafe {
                (*self.spsc.as_ptr())
                    .head
                    .load(Ordering::Acquire)
            };
            if self.tail == self.head_cache + N {
                return Err(QueError::Full);
            }
        }

        // Write value if not full
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

    /// Reserves space for multiple elements. Returns a reservation that must be
    /// committed to publish the values.
    #[inline(always)]
    pub fn reserve(
        &mut self,
        count: usize,
    ) -> Result<Reservation<'_, M, T, N>, QueError> {
        if count == 0 {
            return Err(QueError::InvalidSize);
        }

        if count > N {
            return Err(QueError::InvalidSize);
        }

        let mut available_space = N - (self.tail - self.head_cache);
        if count > available_space {
            self.head_cache = unsafe {
                (*self.spsc.as_ptr())
                    .head
                    .load(Ordering::Acquire)
            };
            available_space = N - (self.tail - self.head_cache);
            if count > available_space {
                return Err(QueError::Full);
            }
        }

        Ok(Reservation {
            start_tail: self.tail,
            producer: self,
            count,
            written: 0,
            committed: false,
        })
    }
}

/// A reservation of space in the queue that can be written to
pub struct Reservation<'a, M: ChannelMode<T>, T, const N: usize> {
    producer: &'a mut Producer<M, T, N>,
    start_tail: usize,
    count: usize,
    written: usize,
    committed: bool,
}

impl<'a, M: ChannelMode<T>, T, const N: usize>
    Reservation<'a, M, T, N>
{
    /// Returns the number of slots reserved
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns the number of values written so far
    #[inline(always)]
    pub fn written(&self) -> usize {
        self.written
    }

    /// Returns the number of slots remaining to be written
    #[inline(always)]
    pub fn remaining(&self) -> usize {
        self.count - self.written
    }

    /// Write the next value in sequence
    ///
    /// # Panics
    /// Panics if all reserved slots have been written
    #[inline(always)]
    pub fn write_next(&mut self, value: T) {
        if self.written >= self.count {
            panic!("Attempted to write beyond reservation limit: written {} of {} slots", 
                   self.written, self.count);
        }

        let index = (self.start_tail + self.written)
            & Producer::<M, T, N>::MODULO_MASK;
        unsafe {
            *(*self.producer.spsc.as_ptr())
                .buffer
                .as_mut_ptr()
                .add(index) = value;
        }

        self.written += 1;
    }

    /// Write all values from a slice
    ///
    /// # Panics
    /// Panics if the slice is larger than the remaining space in the reservation
    #[inline(always)]
    pub fn write_all(&mut self, values: &[T])
    where
        T: Copy,
    {
        let remaining = self.remaining();
        if values.len() > remaining {
            panic!("Attempted to write {} values with only {} slots remaining", 
                   values.len(), remaining);
        }

        if values.is_empty() {
            return;
        }

        let start_index = (self.start_tail + self.written)
            & Producer::<M, T, N>::MODULO_MASK;
        let end_index = start_index + values.len();

        unsafe {
            let buffer_ptr = (*self.producer.spsc.as_ptr())
                .buffer
                .as_mut_ptr();

            if end_index <= N {
                // No wraparound - single copy
                std::ptr::copy_nonoverlapping(
                    values.as_ptr(),
                    buffer_ptr.add(start_index),
                    values.len(),
                );
            } else {
                // Wraparound - two copies
                let first_part_len = N - start_index;

                // Copy first part (until end of buffer)
                std::ptr::copy_nonoverlapping(
                    values.as_ptr(),
                    buffer_ptr.add(start_index),
                    first_part_len,
                );

                // Copy second part (from beginning of buffer)
                std::ptr::copy_nonoverlapping(
                    values.as_ptr().add(first_part_len),
                    buffer_ptr,
                    values.len() - first_part_len,
                );
            }
        }

        self.written += values.len();
    }

    /// Write values from an iterator
    ///
    /// Stops when the reservation is full or the iterator is exhausted.
    /// Returns the number of values written.
    #[inline(always)]
    pub fn write_iter<I>(&mut self, iter: &mut I) -> usize
    where
        I: Iterator<Item = T>,
    {
        let start_written = self.written;

        for value in iter {
            if self.written >= self.count {
                break;
            }
            self.write_next(value);
        }

        self.written - start_written
    }

    /// Commit the reservation, publishing all written values
    ///
    /// If not all reserved slots were written, only the written values are published.
    #[inline(always)]
    pub fn commit(mut self) {
        self.committed = true;

        // Only advance tail by the amount actually written
        self.producer.tail += self.written;
        self.producer.written += self.written;

        // Sync if we've written enough
        if self.producer.written >= burst_amount::<N>() {
            self.producer.sync();
        }
    }

    /// Cancel the reservation without publishing any values
    #[inline(always)]
    pub fn cancel(self) {
        /* cancel is actually a noop lol */
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
