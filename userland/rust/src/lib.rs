#![no_std]

use core::ffi::CStr;
use core::marker::PhantomData;
use core::slice;

#[cfg(feature = "alloc")]
mod allocator;
pub mod cli;
#[cfg(feature = "alloc")]
pub mod directories;
#[cfg(feature = "alloc")]
pub mod ends;
#[cfg(feature = "alloc")]
pub mod links;
pub mod stat;

pub mod ffi {
    pub use __errno_location as __dos_errno_location;
    unsafe extern "C" {
        pub fn read(fd: i32, p: *mut u8, count: usize) -> isize;
        pub fn write(fd: i32, p: *const u8, count: usize) -> isize;
        pub fn open(name: *const i8, flags: i32, ...) -> i32;
        pub fn close(fd: i32) -> i32;
        pub fn lseek(fd: i32, offset: i64, whence: i32) -> i64;
        pub fn __errno_location() -> *mut i32;
        pub fn _exit(status: i32) -> !;
    }
}

/// The kernel keeps the argument vector and its strings on the initial user stack.
#[derive(Clone, Copy)]
pub struct Args<'a> {
    pointers: &'a [*const u8],
    strings: PhantomData<&'a CStr>,
    environment: &'a [*const u8],
}
impl<'a> Args<'a> {
    /// # Safety
    /// `argv` must identify `argc` valid pointers to NUL-terminated strings, all live for `'a`.
    pub unsafe fn from_raw(argc: i32, argv: *const *const u8) -> Self {
        let pointers = if argc <= 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(argv, argc as usize) }
        };
        Self {
            pointers,
            strings: PhantomData,
            environment: &[],
        }
    }
    /// # Safety
    /// The CRT vector includes the argv terminator followed by a null-terminated
    /// envp vector of valid C strings, all live for the duration of main.
    pub unsafe fn from_startup(argc: i32, argv: *const *const u8) -> Self {
        let mut args = unsafe { Self::from_raw(argc, argv) };
        let envp = unsafe { argv.add(args.len() + 1) };
        let mut end = 0;
        while unsafe { !(*envp.add(end)).is_null() } {
            end += 1;
        }
        args.environment = unsafe { slice::from_raw_parts(envp, end) };
        args
    }
    pub fn env(&self, name: &[u8]) -> Option<&'a [u8]> {
        self.environment.iter().find_map(|&p| {
            let entry = unsafe { CStr::from_ptr(p.cast()) }.to_bytes();
            entry.strip_prefix(name)?.strip_prefix(b"=")
        })
    }
    pub fn get(&self, i: usize) -> Option<&'a CStr> {
        let p = *self.pointers.get(i)?;
        Some(unsafe { CStr::from_ptr(p.cast()) })
    }
    pub fn len(&self) -> usize {
        self.pointers.len()
    }
    pub fn is_empty(&self) -> bool {
        self.pointers.is_empty()
    }
    pub fn operands(&self) -> impl Iterator<Item = &'a CStr> + '_ {
        (1..self.len()).filter_map(|i| self.get(i))
    }
}

pub fn errno() -> i32 {
    unsafe { *ffi::__errno_location() }
}
pub fn read(fd: i32, data: &mut [u8]) -> Result<usize, i32> {
    loop {
        let n = unsafe { ffi::read(fd, data.as_mut_ptr(), data.len()) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = errno();
        if e != 4 {
            return Err(e);
        }
    }
}
pub fn write_all(fd: i32, mut data: &[u8]) -> Result<(), i32> {
    while !data.is_empty() {
        let n = unsafe { ffi::write(fd, data.as_ptr(), data.len()) };
        if n < 0 {
            let e = errno();
            if e == 4 {
                continue;
            }
            return Err(e);
        }
        if n == 0 {
            return Err(5);
        }
        data = &data[n as usize..];
    }
    Ok(())
}
pub fn open(name: &CStr, flags: i32) -> Result<i32, i32> {
    let fd = unsafe { ffi::open(name.as_ptr(), flags, 0o666i32) };
    if fd < 0 { Err(errno()) } else { Ok(fd) }
}
pub fn close(fd: i32) -> Result<(), i32> {
    if unsafe { ffi::close(fd) } < 0 {
        Err(errno())
    } else {
        Ok(())
    }
}
pub fn seek(fd: i32, offset: i64, whence: i32) -> Result<u64, i32> {
    let n = unsafe { ffi::lseek(fd, offset, whence) };
    if n < 0 { Err(errno()) } else { Ok(n as u64) }
}
pub fn exit(status: i32) -> ! {
    unsafe { ffi::_exit(status) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    let _ = write_all(2, b"fatal: Rust program panicked\n");
    exit(125)
}

#[macro_export]
macro_rules! utility {
    ($run:path, $name:literal, $help:literal, $informational_status:literal) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
            let args = unsafe { $crate::Args::from_startup(argc, argv) };
            match $crate::cli::information(args, $name, $help, $informational_status) {
                Some(status) => status,
                None => $run(args),
            }
        }
    };
}
