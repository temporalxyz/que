//! [`AtomicUsize`], [`AtomicU64`], and [`Ordering`] from `std` or from `loom`
//! when `cfg(loom)` is set (enabled by the `loom` Cargo feature).

#[cfg(loom)]
pub use loom::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[cfg(not(loom))]
pub use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
