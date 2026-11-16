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

/// A volume has its own offset, capacity and write permission on a shared disk.
#[derive(Clone)]
pub struct BlockWindow {
    disk: alloc::sync::Arc<dyn BlockDevice>,
    start: u64,
    size: u64,
    writable: bool,
}
impl BlockWindow {
    pub fn disk(disk: alloc::sync::Arc<dyn BlockDevice>) -> Self {
        Self {
            start: 0,
            size: disk.size_bytes(),
            writable: disk.is_writable(),
            disk,
        }
    }
    pub fn partition(&self, part: partition::Partition) -> Option<Self> {
        let offset = part.first.checked_mul(512)?;
        let size = part.sectors.checked_mul(512)?;
        if size == 0 || offset.checked_add(size)? > self.size {
            return None;
        }
        Some(Self {
            disk: self.disk.clone(),
            start: self.start.checked_add(offset)?,
            size,
            writable: self.writable && !part.read_only,
        })
    }
    pub fn read_only(mut self) -> Self {
        self.writable = false;
        self
    }
    fn address(&self, at: u64, len: usize) -> Result<u64, Error> {
        if at.checked_add(len as u64).is_none_or(|end| end > self.size) {
            return Err(Error);
        }
        self.start.checked_add(at).ok_or(Error)
    }
}
impl BlockRead for BlockWindow {
    fn size_bytes(&self) -> u64 {
        self.size
    }
    fn read_at(&self, at: u64, out: &mut [u8]) -> Result<(), Error> {
        self.disk.read_at(self.address(at, out.len())?, out)
    }
}
impl BlockDevice for BlockWindow {
    fn is_writable(&self) -> bool {
        self.writable
    }
    fn write_at(&self, at: u64, data: &[u8]) -> Result<(), Error> {
        if !self.writable {
            return Err(Error);
        }
        self.disk.write_at(self.address(at, data.len())?, data)
    }
    fn flush(&self) -> Result<(), Error> {
        if self.writable {
            self.disk.flush()
        } else {
            Ok(())
        }
    }
}
