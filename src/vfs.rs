//! Native pathname and open-file interface. Callers supply their own directory context.
#![forbid(unsafe_code)]
#[path = "vfs/btrfs.rs"]
mod btrfs;
#[path = "vfs/dos.rs"]
pub(crate) mod dos;
#[path = "vfs/driver.rs"]
mod driver;
#[path = "vfs/fat.rs"]
mod fat;
#[path = "vfs/fcb.rs"]
pub(crate) mod fcb;
#[path = "vfs/folders.rs"]
mod folders;
#[path = "vfs/locks.rs"]
pub(crate) mod locks;
#[cfg(not(test))]
#[path = "vfs/ntfs.rs"]
mod ntfs;
#[cfg(not(test))]
#[path = "vfs/readonly.rs"]
mod readonly;
#[path = "vfs/time.rs"]
pub(crate) mod time;
pub(crate) use dos::Directories;
use dosrs_native_posix::{Errno, Open, Result};
/// A mounted object, independent of the path used to open it.
pub(crate) type File = alloc::boxed::Box<dyn Node>;
pub(crate) trait Node: Send {
    fn len(&self) -> Result<u64>;
    fn read(&self, offset: u64, out: &mut [u8]) -> Result<usize>;
    fn write(&self, offset: u64, data: &[u8]) -> Result<()>;
    fn sync(&self) -> Result<()>;
    fn identity(&self) -> (u32, u64);
    fn stamp(&self) -> Result<(u16, u16)>;
    fn set_stamp(&self, time: u16, date: u16) -> Result<()>;
}

pub(crate) fn open(cwd: Directories, path: &str, flags: Open) -> Result<(File, bool)> {
    if flags.truncate && !flags.access.writable() {
        return Err(Errno::Invalid);
    }
    let (dir, tail) = cwd.locate(path.as_bytes()).ok_or(Errno::NoEntry)?;
    driver::at(dir).open(dir, driver::Name::Native(tail), flags)
}
pub(crate) fn unlink(cwd: Directories, path: &str) -> Result<()> {
    let (dir, tail) = cwd.locate(path.as_bytes()).ok_or(Errno::NoEntry)?;
    driver::at(dir).unlink(dir, driver::Name::Native(tail))
}
