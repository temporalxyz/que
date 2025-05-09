use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use bytemuck::AnyBitPattern;

use crate::{
    error::QueError, page_size::PageSize, shmem::Shmem, MAGIC,
};

use super::{burst_amount, Channel};

unsafe impl<T, const N: usize> Send for Consumer<T, N> {}

#[repr(C)]
pub struct Consumer<T, const N: usize> {
    spsc: NonNull<Channel<T, N>>,
    head: usize,
    items_since_last_sync: usize,
    consumer_index: usize,
    last_producer_heartbeat: usize,
}

impl<T: AnyBitPattern, const N: usize> Consumer<T, N> {
    const MODULO_MASK: usize = N - 1;

    /// Joins an existing channel back by shared memory as a consumer.
    pub unsafe fn join_shmem(
        shmem_id: &str,
        #[cfg(target_os = "linux")] page_size: PageSize,
    ) -> Result<Consumer<T, N>, QueError> {
        #[cfg(not(target_os = "linux"))]
        let page_size = PageSize::Standard;

        // Calculate buffer size.
        // If using huge pages, we must uplign to page size.
        let buffer_size: i64 = page_size
            .mem_size(core::mem::size_of::<Channel<T, N>>())
            .try_into()
            .map_err(|_| QueError::InvalidSize)?;

        // Open shmem
        let shmem = Shmem::open_or_create(
            shmem_id,
            buffer_size,
            #[cfg(target_os = "linux")]
            page_size,
        )?;

        unsafe { Consumer::join(shmem.get_mut_ptr()) }
    }

    /// Joins an existing channel backed by `buffer`.
    ///
    ///
    /// SAFETY:
    /// This must point to a buffer of proper size and alignment.
    pub unsafe fn join(
        buffer: *mut u8,
    ) -> Result<Consumer<T, N>, QueError> {
        let buffer = buffer;
        assert!(
            N > 0 && N.is_power_of_two(),
            "Capacity must be a power of two"
        );
        assert!(buffer as usize % 128 == 0, "unaligned");

        // Zerocopy deserialize the SPSC
        let spsc: *const Channel<T, N> = buffer.cast();

        // Check magic
        let magic = (*spsc).magic.load(Ordering::Acquire);
        let capacity = (*spsc).capacity.load(Ordering::Acquire);
        if magic == MAGIC {
            // Check capacity
            if capacity != N {
                return Err(QueError::IncorrectCapacity(capacity));
            }

            // Assume spsc is empty upon joining
            let new_head = (*spsc).tail.load(Ordering::Acquire);
            (*spsc)
                .head
                .store(new_head, Ordering::Release);

            // Successful join if magic and capacity is correct
            Ok(Consumer {
                spsc: NonNull::new_unchecked(buffer.cast()),
                head: new_head,
                items_since_last_sync: 0,
                consumer_index: 0,
                last_producer_heartbeat: (*spsc)
                    .producer_heartbeat
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

    /// Attempts to read the next element. Returns `None` if the
    /// consumer is caught up.
    pub fn pop(&mut self) -> Option<T> {
        // Optimistically read value and then check if valid
        let head_index = self.head & Self::MODULO_MASK;
        let value = unsafe {
            *(*self.spsc.as_ptr())
                .buffer
                .as_ptr()
                .add(head_index)
        };

        // Check if valid
        // 1) is not previously read value
        let tail = unsafe {
            (*self.spsc.as_ptr())
                .tail
                .load(Ordering::Acquire)
        };
        let previously_read_or_uninitialized = tail <= self.head;

        // Nothing else to read
        if previously_read_or_uninitialized {
            return None;
        }

        self.head += 1;
        self.items_since_last_sync += 1;
        self.maybe_sync();
        return Some(value);
    }

    /// Increments the consumer heartbeat.
    ///
    /// Can be read by the producer to see that the consumer is still
    /// online if done periodically. Can also be used to ack individual
    /// messages or alert that we've joined.
    pub fn beat(&self) {
        unsafe {
            (*self.spsc.as_ptr())
                .consumer_heartbeat
                .fetch_add(1, Ordering::Release);
        }
    }

    /// Checks if the producer has incremented its heartbeat since last
    /// called. Can be used by the consumer to see if the producer is
    /// still online if done periodically. Can also be used to ack
    /// individual messages or alert that we've joined.
    pub fn producer_heartbeat(&mut self) -> bool {
        let heartbeat = unsafe {
            (*self.spsc.as_ptr())
                .producer_heartbeat
                .load(Ordering::Acquire)
        };

        if heartbeat != self.last_producer_heartbeat {
            self.last_producer_heartbeat = heartbeat;
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

    #[inline(always)]
    fn maybe_sync(&self) {
        let do_sync = self.items_since_last_sync >= burst_amount::<N>();

        if do_sync {
            unsafe {
                (*self.spsc.as_ptr())
                    .head
                    .store(self.head, Ordering::Release);
            }
        }
    }
}
