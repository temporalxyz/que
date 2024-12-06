use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use bytemuck::AnyBitPattern;

use crate::{
    error::QueError, page_size::PageSize, shmem::Shmem, MAGIC,
};

use super::Channel;

unsafe impl<T, const N: usize> Send for Consumer<T, N> {}

#[repr(C)]
pub struct Consumer<T, const N: usize> {
    spsc: NonNull<Channel<T, N>>,
    head: usize,
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
        let spsc: &mut Channel<T, N> = &mut *buffer.cast();

        // Check magic
        if spsc.magic == MAGIC {
            // Check capacity
            if spsc.capacity != N {
                return Err(QueError::IncorrectCapacity(spsc.capacity));
            }

            // Initialize
            let Channel {
                tail,
                head,
                capacity: _,
                producer_heartbeat: _,
                consumer_heartbeat: _,
                magic: _,
                buffer: _unused,
            } = spsc;

            // Assume spsc is empty upon joining
            let new_head = tail.load(Ordering::Acquire);
            head.store(new_head, Ordering::Release);

            // Successful join if magic and capacity is correct
            Ok(Consumer {
                spsc: NonNull::new_unchecked(spsc),
                head: new_head,
                consumer_index: 0,
                last_producer_heartbeat: spsc
                    .producer_heartbeat
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

    /// Attempts to read the next element. Returns `None` if the consuemr is caught up.
    pub fn pop(&mut self) -> Option<T> {
        let spsc = unsafe { self.spsc.as_mut() };

        // Optimistically read value and then check if valid
        let head_index = self.head & Self::MODULO_MASK;
        let value = unsafe { *spsc.buffer.as_ptr().add(head_index) };

        // Check if valid
        // 1) is not previously read value
        let tail = spsc.tail.load(Ordering::Acquire);
        let previously_read_or_uninitialized = tail <= self.head;

        // Nothing else to read
        if previously_read_or_uninitialized {
            return None;
        }

        self.head += 1;
        return Some(value);
    }

    /// Increments the consumer heartbeat.
    ///
    /// Can be read by the producer to see that the consumer is still online if done periodically. Can also be used to ack individual messages or alert that we've joined.
    pub fn beat(&self) {
        unsafe {
            self.spsc
                .as_ref()
                .consumer_heartbeat
                .fetch_add(1, Ordering::Release);
        }
    }

    /// Checks if the producer has incremented its heartbeat since last called. Can be used by the consumer to see if the producer is still online if done periodically. Can also be used to ack individual messages or alert that we've joined.
    pub fn producer_heartbeat(&mut self) -> bool {
        let heartbeat = unsafe {
            self.spsc
                .as_ref()
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
}
