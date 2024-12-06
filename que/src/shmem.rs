use std::{
    ffi::{c_void, CString},
    num::NonZeroUsize,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    ptr::NonNull,
};

use nix::{
    errno::Errno,
    fcntl::{open, OFlag},
    libc::{c_char, munmap, shm_unlink, unlink, S_IRUSR, S_IWUSR},
    sys::{
        mman::{shm_open, MapFlags, ProtFlags},
        stat::Mode,
    },
    unistd::ftruncate,
};

use crate::page_size::PageSize;

#[derive(Clone)]
pub struct Shmem {
    pub id: String,
    pub(crate) size: i64,
    pub fd: i32,
    addr: NonNull<()>,
    page_size: PageSize,
}

impl Shmem {
    /// Open or create a shmem
    ///
    /// NOTE:
    /// If using huge pages, expected at path /mnt/hugepages/ or
    /// /mnt/gigantic/
    pub fn open_or_create(
        id: &str,
        uplined_size: i64,
        #[cfg(target_os = "linux")] page_size: PageSize,
    ) -> Result<Shmem, ShmemError> {
        #[cfg(not(target_os = "linux"))]
        let page_size = PageSize::Standard;

        // Open with read + write privileges
        let mode = Mode::from_bits(S_IRUSR | S_IWUSR).unwrap();
        let fd = if page_size.is_gigantic() {
            // Huge pages expected at path /mnt/gigantic/...
            let path = format!("/mnt/gigantic/{}", id);
            let fd = open::<str>(
                &path,
                OFlag::O_RDWR | OFlag::O_CREAT,
                mode,
            )?;

            unsafe { OwnedFd::from_raw_fd(fd) }
        } else if page_size.is_huge() {
            // Huge pages expected at path /mnt/hugepages/...
            let path = format!("/mnt/hugepages/{}", id);
            let fd = open::<str>(
                &path,
                OFlag::O_RDWR | OFlag::O_CREAT,
                mode,
            )?;

            unsafe { OwnedFd::from_raw_fd(fd) }
        } else {
            // Default back to shm
            let path = CString::new(id).unwrap();

            let fd = shm_open(
                path.as_c_str(),
                OFlag::O_RDWR | OFlag::O_CREAT,
                mode,
            )?;
            ftruncate(&fd, uplined_size)?;
            fd
        };

        // Add huge pages if specified
        #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
        let mut map_flags = MapFlags::MAP_SHARED;
        #[cfg(target_os = "linux")]
        if page_size.is_huge() {
            map_flags |= MapFlags::MAP_HUGETLB;
        }
        if page_size.is_gigantic() {
            map_flags |= MapFlags::MAP_HUGETLB;
            map_flags |= MapFlags::MAP_HUGE_1GB;
        }

        let addr = unsafe {
            nix::sys::mman::mmap(
                None,
                NonZeroUsize::new_unchecked(uplined_size as usize),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                map_flags,
                &fd,
                0,
            )
        };
        let addr = match addr {
            Ok(addr) => addr.cast(),
            Err(e) => return Err(ShmemError::Errno(e as i32)),
        };
        Ok(Shmem {
            id: id.to_string(),
            size: uplined_size,
            fd: fd.as_raw_fd(),
            addr,
            page_size,
        })
    }

    /// Closes the shared memory region (unmap, unlink, close)
    pub fn close(self) -> Result<(), ShmemError> {
        // # Safety
        //
        // if current process is the owner of the shared_memory,i.e.
        // creator of the shared memory, then it should clean up
        // after. the procedure is as follow:
        // 1. unmap the shared memory from processes virtual address
        //    space.
        // 2. unlink the shared memory completely from the os if self is
        //    the owner
        // 3. close the file descriptor of the shared memory
        println!("Unmapping shared memory: {}", self.id);

        let res = unsafe {
            munmap(
                self.addr.as_ptr() as *mut c_void,
                self.size as usize,
            )
        };
        if res != 0 {
            let err = std::io::Error::last_os_error();
            println!("failed to unmap shared memory from the virtual memory space: {res} -> {err:?}")
        }

        println!("Closing shared memory: {}", self.id);

        if self.page_size.is_gigantic() {
            // Remove the huge page file from /mnt/gigantic if it
            // exists
            let path = format!("/mnt/gigantic/{}", self.id);
            let c_path = CString::new(path).unwrap();
            if unsafe { unlink(c_path.as_ptr()) } != 0 {
                return Err(ShmemError::UnlinkError);
            }
        } else if self.page_size.is_huge() {
            // Remove the huge page file from /mnt/hugepages if it
            // exists
            let path = format!("/mnt/hugepages/{}", self.id);
            let c_path = CString::new(path).unwrap();
            if unsafe { unlink(c_path.as_ptr()) } != 0 {
                return Err(ShmemError::UnlinkError);
            }
        } else {
            let storage_id: *const c_char =
                self.id.as_bytes().as_ptr() as *const c_char;
            if unsafe { shm_unlink(storage_id) } != 0 {
                println!("failed to reclaim shared memory")
            }
        }

        println!("fd closed: {}", self.id);

        Ok(())
    }

    /// Returns a raw pointer to the shared memory region
    pub fn get_mut_ptr(&self) -> *mut u8 {
        self.addr.as_ptr() as *mut u8
    }
}

/// Cleans up a shared memory region by opening it and then closing it
pub fn cleanup_shmem(
    id: &str,
    size: i64,
    #[cfg(target_os = "linux")] huge_pages: PageSize,
) -> Result<(), ShmemError> {
    let shmem = Shmem::open_or_create(
        id,
        size,
        #[cfg(target_os = "linux")]
        huge_pages,
    )?;

    shmem.close()
}

#[derive(Clone, Copy)]
pub enum ShmemError {
    BadFileDescriptor,
    AllocationFailedErr,
    InvalidPermissions,
    UnlinkError,
    Errno(i32),
}

impl std::error::Error for ShmemError {}

impl core::fmt::Debug for ShmemError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            ShmemError::BadFileDescriptor => {
                f.write_str("Bad file descriptor for shared memory object")
            }
            ShmemError::AllocationFailedErr => {
                f.write_str("Failed to allocate shared memory. If using huge/gigantic pages, have you preallocated pages?")
            }
            ShmemError::InvalidPermissions => {
                f.write_str("Invalid permissions for opening/mmaping shared memory object")
            }
            ShmemError::UnlinkError => {
                f.write_str("Failed to unlink huge page")
            }
            ShmemError::Errno(e) => write!(f, "Other system error: {}", e),
        }
    }
}

impl core::fmt::Display for ShmemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<Errno> for ShmemError {
    fn from(e: Errno) -> Self {
        match e {
            Errno::EBADF => ShmemError::BadFileDescriptor,
            Errno::ENOMEM => ShmemError::AllocationFailedErr,
            Errno::EACCES => ShmemError::InvalidPermissions,
            e => ShmemError::Errno(e as i32),
        }
    }
}
