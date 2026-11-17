//! Enumerate mounted volumes by driver identity, using their canonical path.
//! Each record contains source, mount point, type and options as four NUL-terminated fields.
#[path = "mounts/directory.rs"]
mod directory;
use super::{Directories, btrfs, dos, driver};
pub(crate) use directory::{MountDir, qualify};
use dosrs_native_posix::{Errno, Result};

pub(crate) const RECORD_SIZE: usize = 96;

#[derive(Clone, Copy, PartialEq)]
enum Volume {
    Boot,
    Hard,
    Btrfs,
}

impl Volume {
    fn at(directory: u32) -> Self {
        if directory & driver::HARD == 0 {
            return Self::Boot;
        }
        #[cfg(not(test))]
        if driver::btrfs_drive() {
            return Self::Btrfs;
        }
        Self::Hard
    }
}

pub(crate) fn entry(index: usize, cwd: Directories, out: &mut [u8; RECORD_SIZE]) -> Result<usize> {
    let root = if btrfs::is_mounted() {
        Volume::Btrfs
    } else {
        Volume::at(cwd.locate(b"/").ok_or(Errno::NoEntry)?.0)
    };
    let mut entries = [(root, b"/".as_slice()); 3];
    let mut count = 1;
    for (letter, path) in [(0, b"/dos/a".as_slice()), (2, b"/dos/c")] {
        let Some(dir) = dos::drive_root(letter) else {
            continue;
        };
        let volume = Volume::at(dir);
        if volume != root {
            entries[count] = (volume, path);
            count += 1;
        }
    }
    let Some(&(volume, path)) = entries[..count].get(index) else {
        return Ok(0);
    };
    let mut source = [0u8; 41];
    let (name, kind, writable) = if volume == Volume::Btrfs {
        let (fsid, writable) = btrfs::mount_identity().ok_or(Errno::Io)?;
        source[..5].copy_from_slice(b"UUID=");
        let mut at = 5;
        let hex = b"0123456789abcdef";
        for (i, byte) in fsid.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                source[at] = b'-';
                at += 1;
            }
            source[at] = hex[(byte >> 4) as usize];
            source[at + 1] = hex[(byte & 15) as usize];
            at += 2;
        }
        (&source[..at], b"btrfs".as_slice(), writable)
    } else {
        let (dir, name) = if volume == Volume::Boot {
            (0, b"A:")
        } else {
            (driver::HARD, b"C:")
        };
        let backend = driver::at(dir);
        let (_, _, fs) = backend.volume(dir).ok_or(Errno::Io)?;
        let kind = if fs.starts_with(b"FAT") {
            b"vfat".as_slice()
        } else if fs.starts_with(b"NTFS") {
            b"ntfs".as_slice()
        } else {
            return Err(Errno::NotSupported);
        };
        (name.as_slice(), kind, !backend.read_only())
    };
    let mut offset = 0;
    for field in [name, path, kind, if writable { b"rw" } else { b"ro" }] {
        out[offset..offset + field.len()].copy_from_slice(field);
        offset += field.len();
        out[offset] = 0;
        offset += 1;
    }
    Ok(offset)
}
