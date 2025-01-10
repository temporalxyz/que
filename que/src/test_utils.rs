//! Need 128 byte aligned allocs for tests

use std::alloc::Layout;

use bytemuck::AnyBitPattern;

use crate::Channel;

pub(crate) struct Alloc {
    pub ptr: *mut u8,
    pub layout: Layout,
}

impl Alloc {
    pub(crate) fn new(size: usize, align: usize) -> Alloc {
        let layout = Layout::from_size_align(size, align).unwrap();

        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            panic!("alloc failed during test")
        }
        Alloc { ptr, layout }
    }
}

impl Drop for Alloc {
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.ptr, self.layout);
        }
    }
}

pub(crate) fn new_spsc_buffer<T: AnyBitPattern, const N: usize>(
) -> Alloc {
    let buffer_size = std::mem::size_of::<Channel<T, N>>();
    Alloc::new(buffer_size, 128)
}
