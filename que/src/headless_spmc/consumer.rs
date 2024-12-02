use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use bytemuck::AnyBitPattern;

use crate::{
    error::QueError, headless_spmc::MAGIC, page_size::PageSize,
    shmem::Shmem,
};

use super::{burst_amount, Channel};

unsafe impl<T, const N: usize> Send for Consumer<T, N> {}

#[repr(C)]
pub struct Consumer<T, const N: usize> {
    spsc: NonNull<Channel<T, N>>,
    head: usize,
    interval: usize,
    consumer_index: usize,
    last_producer_heartbeat: usize,
}

impl<T: AnyBitPattern, const N: usize> Consumer<T, N> {
    const MODULO_MASK: usize = N - 1;

    // Joins an existing shmem. Does not use huge or gigantic pages.
    pub unsafe fn join_shmem(
        shmem_id: &str,
        #[cfg(target_os = "linux")] page_size: PageSize,
    ) -> Result<Consumer<T, N>, QueError> {
        Self::join_shmem_multi(
            shmem_id,
            #[cfg(target_os = "linux")]
            page_size,
            1,
        )
    }

    // Joins an existing shmem. Does not use huge or gigantic pages.
    pub unsafe fn join_shmem_multi(
        shmem_id: &str,
        #[cfg(target_os = "linux")] page_size: PageSize,
        interval: usize,
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

        Consumer::join_multi(shmem.get_mut_ptr(), interval)
    }

    pub unsafe fn join(
        buffer: *mut u8,
    ) -> Result<Consumer<T, N>, QueError> {
        Self::join_multi(buffer, 1)
    }

    pub unsafe fn join_multi(
        buffer: *mut u8,
        interval: usize,
    ) -> Result<Consumer<T, N>, QueError> {
        assert!(
            N > 0 && N.is_power_of_two(),
            "Capacity must be a power of two"
        );
        assert!(buffer as usize % 128 == 0, "unaligned");
        assert!(
            interval <= 64,
            "interval must be less than or equal to 64"
        );

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
                head: _, // not used in headless mode
                capacity: _,
                producer_heartbeat: _,
                consumer_heartbeat,
                magic: _,
                buffer: _unused,
            } = spsc;

            // Assume spsc is empty upon joining
            let head = tail.load(Ordering::Acquire);
            consumer_heartbeat.fetch_add(1, Ordering::Release);

            // Successful join if magic and capacity is correct
            Ok(Consumer {
                spsc: NonNull::new_unchecked(spsc),
                head: next_modulo(head, 0, interval),
                interval,
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
            println!("magic = {}; expected {}", spsc.magic, MAGIC);
            Err(QueError::CorruptionDetected)
        }
    }

    /// Returns none if consumer_index would be equal to interval
    pub fn next_multi(&self) -> Option<Consumer<T, N>> {
        if self.consumer_index + 1 == self.interval {
            None
        } else {
            Some(Consumer {
                consumer_index: self.consumer_index + 1,
                head: self.head + 1,
                ..*self
            })
        }
    }

    pub fn pop(&mut self) -> Option<T> {
        let spsc = unsafe { self.spsc.as_mut() };

        // Optimistically read value and then check if valid
        loop {
            let head_index = self.head & Self::MODULO_MASK;
            let value =
                unsafe { *spsc.buffer.as_ptr().add(head_index) };

            // Check if valid
            // 1) is not overrun
            // 2) is not previously read value
            let tail = spsc.tail.load(Ordering::Acquire);
            let not_overrun = tail
                <= (self
                    .head
                    .wrapping_add(N - burst_amount::<N>()));
            let previously_read_or_uninitialized = tail <= self.head;

            // Nothing else to read
            if previously_read_or_uninitialized {
                return None;
            }

            // If overrun, update head and try again
            if !not_overrun {
                // Must reset to next integer that is consumer_index %
                // interval
                self.head = next_modulo(
                    tail.wrapping_sub(N - burst_amount::<N>()),
                    self.consumer_index,
                    self.interval,
                );
                continue;
            }

            self.head += self.interval;
            return Some(value);
        }
    }

    pub fn beat(&self) {
        unsafe {
            self.spsc
                .as_ref()
                .consumer_heartbeat
                .fetch_add(1, Ordering::Release);
        }
    }

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

#[inline(always)]
fn next_modulo(
    head: usize,
    target_mod: usize,
    mod_value: usize,
) -> usize {
    let head_mod = head % mod_value;
    let add_value = (target_mod + mod_value - head_mod) % mod_value;
    head + add_value
}
