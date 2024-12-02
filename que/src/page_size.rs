#[inline(always)]
pub const fn get_upligned_size(page_size: usize, size: usize) -> usize {
    page_size * size.div_ceil(page_size)
}

#[derive(Clone, Copy)]
#[repr(C)]
pub enum PageSize {
    /// Default system page size (typically 4KiB)
    Standard,
    /// 2MiB
    #[cfg(target_os = "linux")]
    Huge,
    /// 1GiB
    #[cfg(target_os = "linux")]
    Gigantic,
}

impl PageSize {
    /// 1 GiB
    pub const GIGANTIC: usize = 1 << 30;

    /// 2 MiB
    pub const HUGE: usize = 1 << 21;

    /// Returns the required buffer size for the selected page size
    ///
    /// [PageSize::Standard]: input `size`
    ///
    /// [PageSize::Huge] or [PageSize::Gigantic]: rounded up to the
    /// nearest page size.
    pub fn mem_size(&self, size: usize) -> usize {
        match self {
            // shmem can be truncated arbitrarily
            PageSize::Standard => size,
            PageSize::Huge => get_upligned_size(Self::GIGANTIC, size),
            PageSize::Gigantic => get_upligned_size(Self::HUGE, size),
        }
    }

    /// Returns `true` if [PageSize::Huge]
    #[inline(always)]
    pub fn is_huge(&self) -> bool {
        matches!(self, PageSize::Huge)
    }

    /// Returns `true` if [PageSize::Gigantic]
    #[inline(always)]
    pub fn is_gigantic(&self) -> bool {
        matches!(self, PageSize::Gigantic)
    }
}
