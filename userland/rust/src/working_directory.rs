//! Checked libc access to the process working directory.
extern crate alloc;
use alloc::{vec, vec::Vec};
use core::ffi::CStr;

unsafe extern "C" {
    fn chdir(path: *const i8) -> i32;
    fn fchdir(fd: i32) -> i32;
    fn getcwd(buffer: *mut i8, size: usize) -> *mut i8;
}

pub struct SavedDirectory(i32);

impl SavedDirectory {
    pub fn current() -> Result<Self, i32> {
        const O_SEARCH: i32 = 0x200000;
        const O_DIRECTORY: i32 = 0x10000;
        const O_CLOEXEC: i32 = 0x80000;
        crate::open(c".", O_SEARCH | O_DIRECTORY | O_CLOEXEC).map(Self)
    }

    pub fn restore(&self) -> Result<(), i32> {
        // The descriptor pins the directory even if its name changes.
        if unsafe { fchdir(self.0) } < 0 {
            Err(crate::errno())
        } else {
            Ok(())
        }
    }
}

impl Drop for SavedDirectory {
    fn drop(&mut self) {
        let _ = crate::close(self.0);
    }
}

pub fn change(path: &CStr) -> Result<(), i32> {
    // The borrowed CStr remains live throughout the call.
    if unsafe { chdir(path.as_ptr()) } < 0 {
        Err(crate::errno())
    } else {
        Ok(())
    }
}

pub fn current() -> Result<Vec<u8>, i32> {
    let mut bytes = vec![];
    let mut size = 256;
    loop {
        bytes
            .try_reserve_exact(size - bytes.len())
            .map_err(|_| 12)?;
        bytes.resize(size, 0);
        // libc writes at most size bytes into this initialized allocation.
        if !unsafe { getcwd(bytes.as_mut_ptr().cast(), bytes.len()) }.is_null() {
            let end = bytes.iter().position(|&b| b == 0).ok_or(5)?;
            bytes.truncate(end);
            return Ok(bytes);
        }
        let error = crate::errno();
        if error != 34 {
            return Err(error);
        }
        size = size.checked_mul(2).ok_or(12)?;
    }
}
