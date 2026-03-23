use std::ops::{Deref, DerefMut};
use std::sync::atomic::Ordering;
use std::{ptr::NonNull, sync::Arc};

use bytemuck::AnyBitPattern;
use derivative::Derivative;

use crate::{
    error::QueError, page_size::PageSize, shmem::Shmem, ChannelMode,
    ShmemMode, MAGIC,
};

use super::{burst_amount, Channel};

unsafe impl<M: ChannelMode<T>, T, const N: usize> Send
    for Consumer<M, T, N>
{
}

#[repr(C)]
pub struct Consumer<M: ChannelMode<T>, T, const N: usize> {
    spsc: NonNull<Channel<M, T, N>>,
    head: usize,
    items_since_last_sync: usize,
    consumer_index: usize,
    last_producer_heartbeat: usize,
}

impl<T: AnyBitPattern, const N: usize> Consumer<ShmemMode, T, N> {
    /// Joins an existing channel back by shared memory as a consumer.
    pub unsafe fn join_shmem(
        shmem_id: &str,
        #[cfg(target_os = "linux")] page_size: PageSize,
    ) -> Result<Consumer<ShmemMode, T, N>, QueError> {
        #[cfg(not(target_os = "linux"))]
        let page_size = PageSize::Standard;

        // Calculate buffer size.
        // If using huge pages, we must uplign to page size.
        let buffer_size: i64 = page_size
            .mem_size(core::mem::size_of::<Channel<ShmemMode, T, N>>())
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
}

#[derive(Derivative)]
#[derivative(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Element<'a, M: ChannelMode<T>, T, const N: usize> {
    value: &'a mut T,
    #[derivative(Debug = "ignore")]
    #[derivative(PartialEq = "ignore")]
    #[derivative(PartialOrd = "ignore")]
    #[derivative(Ord = "ignore")]
    consumer: &'a mut Consumer<M, T, N>,
}

impl<'a, M: ChannelMode<T>, T: PartialEq, const N: usize> PartialEq<T>
    for Element<'a, M, T, N>
{
    fn eq(&self, other: &T) -> bool {
        self.deref().eq(other)
    }
}
impl<'a, M: ChannelMode<T>, T: PartialOrd, const N: usize> PartialOrd<T>
    for Element<'a, M, T, N>
{
    fn partial_cmp(&self, other: &T) -> Option<std::cmp::Ordering> {
        self.deref().partial_cmp(other)
    }
}

impl<'a, M: ChannelMode<T>, T, const N: usize> Drop
    for Element<'a, M, T, N>
{
    fn drop(&mut self) {
        (*self.consumer).head += 1;
        (*self.consumer).items_since_last_sync += 1;
        (*self.consumer).maybe_sync();
    }
}

impl<'a, M: ChannelMode<T>, T, const N: usize> Deref
    for Element<'a, M, T, N>
{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'a, M: ChannelMode<T>, T, const N: usize> DerefMut
    for Element<'a, M, T, N>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

impl<M: ChannelMode<T>, T, const N: usize> Consumer<M, T, N> {
    const MODULO_MASK: usize = N - 1;
    /// Joins an existing channel backed by `buffer`.
    ///
    ///
    /// SAFETY:
    /// This must point to a buffer of proper size and alignment.
    ///
    /// In LocalMode, must point to a region allocated by an Arc with the strong count already incremented!
    pub unsafe fn join(
        buffer: *mut u8,
    ) -> Result<Consumer<M, T, N>, QueError> {
        let buffer = buffer;
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

            if M::BACKED_BY_ARCC {
                unsafe {
                    Arc::increment_strong_count(spsc);
                }
            }

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
        let head_index = self.head & Self::MODULO_MASK;
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
        let value = unsafe {
            core::ptr::read(
                (*self.spsc.as_ptr())
                    .buffer
                    .as_ptr()
                    .add(head_index),
            )
        };

        self.head += 1;
        self.items_since_last_sync += 1;
        self.maybe_sync();
        return Some(value);
    }

    /// Attempts to read the next element. Returns `None` if the
    /// consumer is caught up.
    pub fn pop_zerocopy<'a>(
        &'a mut self,
    ) -> Option<Element<'a, M, T, N>> {
        let head_index = self.head & Self::MODULO_MASK;
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
        let value: &mut T = unsafe {
            &mut *(*self.spsc.as_ptr())
                .buffer
                .as_mut_ptr()
                .add(head_index)
        };
        let element = Element {
            value,
            consumer: self,
        };

        return Some(element);
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
    fn maybe_sync(&mut self) {
        let do_sync = self.items_since_last_sync >= burst_amount::<N>();

        if do_sync {
            self.items_since_last_sync = 0;
            unsafe {
                (*self.spsc.as_ptr())
                    .head
                    .store(self.head, Ordering::Release);
            }
        }
    }
}

impl<M: ChannelMode<T>, T, const N: usize> Drop for Consumer<M, T, N> {
    fn drop(&mut self) {
        // LocalMode is backed by arc
        if M::BACKED_BY_ARCC {
            unsafe { drop(Arc::from_raw(self.spsc.as_ptr())) }
        }
    }
}
