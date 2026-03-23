use std::ops::{Deref, DerefMut};

use crate::atomic_compat::AtomicUsize;

/// Simple 128-byte aligned wrapper around an `AtomicUsize` to prevent
/// false sharing.
#[derive(Default)]
#[repr(C, align(128))]
pub(crate) struct CachePaddedAtomicUsize {
    inner: AtomicUsize,
}

impl Deref for CachePaddedAtomicUsize {
    type Target = AtomicUsize;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for CachePaddedAtomicUsize {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl CachePaddedAtomicUsize {
    #[cfg(all(loom, test))]
    pub(crate) fn new(value: usize) -> Self {
        Self {
            inner: AtomicUsize::new(value),
        }
    }
}
