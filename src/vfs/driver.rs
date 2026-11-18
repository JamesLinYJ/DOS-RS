//! Volume dispatch shared by DOS, native processes and loaders.
#[cfg(not(test))]
use super::btrfs;
use super::dos::{DirMatch, DriveBlock};
use super::{File, fat};
use crate::kernel_cell::KernelCell;
use dosrs_native_posix::{Open, Result};
pub type VolumeId = ([u8; 4], [u8; 11], [u8; 8]);
pub enum Name<'a> {
    Dos(&'a [u8; 11]),
    Native(&'a [u8]),
}
impl Name<'_> {
    pub fn with_bytes<T>(self, apply: impl FnOnce(&[u8], bool) -> T) -> T {
        match self {
            Self::Dos(name) => apply(&super::dos::short_name(name), true),
            Self::Native(path) => apply(path, false),
        }
    }
}
pub trait Volume: Sync {
    fn read_only(&self) -> bool {
        false
    }
    fn open(&self, dir: u32, path: Name, flags: Open) -> Result<(File, bool)>;
    fn unlink(&self, _: u32, _: Name) -> Result<()> {
        Err(dosrs_native_posix::Errno::ReadOnly)
    }
    fn child(&self, dir: u32, name: &[u8; 11]) -> Option<u32>;
    fn parent(&self, dir: u32) -> u32;
    fn path(&self, dir: u32, out: &mut [u8]) -> usize;
    fn find(&self, dir: u32, pattern: &[u8; 11], attr: u8, start: u16) -> Option<DirMatch>;
    fn attr(&self, dir: u32, name: &[u8; 11]) -> Option<u8>;
    fn set_attr(&self, _: u32, _: &[u8; 11], _: u8) -> bool {
        false
    }
    fn replace(&self, _: u32, _: &[u8; 11], _: &[u8], _: u8) -> bool {
        false
    }

    fn mkdir(&self, _: u32, _: &[u8; 11]) -> bool {
        false
    }
    fn rmdir(&self, _: u32, _: &[u8; 11]) -> bool {
        false
    }
    fn rename(&self, _: u32, _: &[u8; 11], _: &[u8; 11]) -> bool {
        false
    }
    fn media(&self, dir: u32) -> u8;
    fn volume(&self, dir: u32) -> Option<VolumeId>;
    fn set_volume(&self, _: u32, _: &[u8; 4], _: &[u8; 11]) -> bool {
        false
    }
    fn parameters(&self, dir: u32) -> Option<DriveBlock>;
    fn space(&self, dir: u32) -> Option<(u32, u32, u32, u32)>;
    fn flush(&self, dir: u32) -> bool;
}
/// The low bits are private to the driver and identify a directory within its volume.
pub(super) const HARD: u32 = 1 << 31;
#[cfg(not(test))]
static BTRFS_DRIVE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
#[cfg(not(test))]
pub(super) fn btrfs_drive() -> bool {
    BTRFS_DRIVE.load(core::sync::atomic::Ordering::Relaxed)
}
static HARD_DRIVER: KernelCell<Option<&'static dyn Volume>> = KernelCell::new(None);

pub fn boot() {
    fat::boot();
    #[cfg(not(test))]
    if !hard_present() {
        if let Some(ata) = crate::bios::ata_volume::AtaVolume::primary() {
            mount_disk(dosrs_block::BlockWindow::disk(alloc::sync::Arc::new(ata)));
        }
    }
}
#[cfg(not(test))]
fn mount_disk(device: dosrs_block::BlockWindow) {
    let windows = match dosrs_block::partition::data_partitions(&device) {
        Ok(None) => alloc::vec![device],
        Ok(Some(parts)) => parts
            .into_iter()
            .filter_map(|p| device.partition(p))
            .collect(),
        Err(_) => return,
    };
    // A DOS system disk may have a separate Btrfs partition for POSIX /.
    // Each driver owns one mount. Avoid replacing it on subsequent partitions.
    let root = windows
        .iter()
        .find_map(|w| btrfs::mount(w.clone()).then_some(&btrfs::Driver as &dyn Volume));
    let hard = windows
        .iter()
        .find_map(|w| {
            use dosrs_block::BlockDevice;
            if !fat::mount(w.clone()) {
                return None;
            }
            if w.is_writable() {
                crate::puts("Mounted persistent FAT volume C:\\ on IDE.\n");
                Some(&fat::Driver as &dyn Volume)
            } else {
                crate::puts("Mounted FAT volume C:\\ (read-only).\n");
                Some(&super::readonly::FAT as &dyn Volume)
            }
        })
        .or_else(|| {
            windows
                .into_iter()
                .find_map(|w| super::ntfs::mount(w).then_some(&super::ntfs::Driver as &dyn Volume))
        });
    *HARD_DRIVER.borrow() = hard.or(root);
    if root.is_some() && hard.is_none() {
        BTRFS_DRIVE.store(true, core::sync::atomic::Ordering::Relaxed);
        crate::puts(if btrfs::is_writable() {
            "Mounted Btrfs C:\\ (read/write).\n"
        } else {
            "Mounted Btrfs C:\\ (read-only).\n"
        });
    }
}
pub fn hard_present() -> bool {
    HARD_DRIVER.borrow().is_some()
}
pub fn at(dir: u32) -> &'static dyn Volume {
    if dir & HARD != 0 {
        return HARD_DRIVER
            .borrow()
            .expect("directory belongs to an unmounted drive");
    }
    &fat::Driver
}
