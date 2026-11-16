//! Random-access disk I/O supplied by the mounting kernel.
#![no_std]
#![forbid(unsafe_code)]

#[derive(Debug)]
pub struct Error;
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("block device I/O failed")
    }
}
pub trait BlockRead: Send + Sync {
    fn read_at(&self, offset: u64, out: &mut [u8]) -> Result<(), Error>;
    fn size_bytes(&self) -> u64;
}
pub trait BlockDevice: BlockRead {
    fn is_writable(&self) -> bool;
    fn write_at(&self, offset: u64, data: &[u8]) -> Result<(), Error>;
    fn flush(&self) -> Result<(), Error>;
}

extern crate alloc;
pub mod partition;
