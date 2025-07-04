use std::alloc::Layout;

use crate::{Channel, LocalMode};

pub struct Alloc {
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

pub(crate) fn new_spsc_buffer<T, const N: usize>() -> Alloc {
    let buffer_align = std::mem::align_of::<Channel<LocalMode, T, N>>();
    let buffer_size = std::mem::size_of::<Channel<LocalMode, T, N>>();
    Alloc::new(buffer_size, buffer_align)
}
