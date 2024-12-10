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
            PageSize::Standard => {
                get_upligned_size(PageSize::standard(), size)
            }
            #[cfg(target_os = "linux")]
            PageSize::Huge => get_upligned_size(Self::HUGE, size),
            #[cfg(target_os = "linux")]
            PageSize::Gigantic => {
                get_upligned_size(Self::GIGANTIC, size)
            }
        }
    }

    /// Returns `true` if [PageSize::Huge]
    #[inline(always)]
    pub fn is_huge(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            matches!(self, PageSize::Huge)
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    /// Returns `true` if [PageSize::Gigantic]
    #[inline(always)]
    pub fn is_gigantic(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            matches!(self, PageSize::Gigantic)
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    #[inline(always)]
    pub fn standard() -> usize {
        nix::unistd::sysconf(nix::unistd::SysconfVar::PAGE_SIZE)
            .expect("unable to get default page size")
            .expect("unable to get default page size")
            .try_into()
            .unwrap()
    }
}
