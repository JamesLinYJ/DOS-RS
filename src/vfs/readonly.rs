//! A read-only partition denies mutations even through metadata calls on an open handle.
use super::{
    File, Node,
    dos::{DirMatch, DriveBlock},
    driver::{Name, Volume, VolumeId},
};
use alloc::boxed::Box;
use dosrs_native_posix::{Errno, Open, Result};
pub(super) static FAT: ReadOnly = ReadOnly(&super::fat::Driver);
pub(super) struct ReadOnly(&'static dyn Volume);
struct ReadFile(File);
impl Node for ReadFile {
    fn len(&self) -> Result<u64> {
        self.0.len()
    }
    fn read(&self, pos: u64, out: &mut [u8]) -> Result<usize> {
        self.0.read(pos, out)
    }
    fn write(&self, _: u64, _: &[u8]) -> Result<()> {
        Err(Errno::ReadOnly)
    }
    fn sync(&self) -> Result<()> {
        Ok(())
    }
    fn identity(&self) -> (u32, u64) {
        self.0.identity()
    }
    fn stamp(&self) -> Result<(u16, u16)> {
        self.0.stamp()
    }
    fn set_stamp(&self, _: u16, _: u16) -> Result<()> {
        Err(Errno::ReadOnly)
    }
}
impl Volume for ReadOnly {
    fn read_only(&self) -> bool {
        true
    }

    fn open(&self, dir: u32, name: Name, flags: Open) -> Result<(File, bool)> {
        if flags.access.writable() || flags.create || flags.truncate {
            return Err(Errno::ReadOnly);
        }
        let (file, _) = self.0.open(dir, name, flags)?;
        Ok((Box::new(ReadFile(file)), false))
    }
    fn child(&self, dir: u32, name: &[u8; 11]) -> Option<u32> {
        self.0.child(dir, name)
    }
    fn parent(&self, dir: u32) -> u32 {
        self.0.parent(dir)
    }
    fn path(&self, dir: u32, out: &mut [u8]) -> usize {
        self.0.path(dir, out)
    }
    fn find(&self, dir: u32, pattern: &[u8; 11], attr: u8, start: u16) -> Option<DirMatch> {
        self.0.find(dir, pattern, attr, start)
    }
    fn attr(&self, dir: u32, name: &[u8; 11]) -> Option<u8> {
        self.0.attr(dir, name)
    }
    fn media(&self, dir: u32) -> u8 {
        self.0.media(dir)
    }
    fn volume(&self, dir: u32) -> Option<VolumeId> {
        self.0.volume(dir)
    }
    fn parameters(&self, dir: u32) -> Option<DriveBlock> {
        self.0.parameters(dir)
    }
    fn space(&self, dir: u32) -> Option<(u32, u32, u32, u32)> {
        self.0.space(dir)
    }
    fn flush(&self, _: u32) -> bool {
        true
    }
}
