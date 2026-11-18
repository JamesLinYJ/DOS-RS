//! Seed device inodes in a newly formatted installation volume.
#![forbid(unsafe_code)]

use dosrs_block::{BlockDevice, BlockRead, Error};
use dosrs_btrfs::{Filesystem, inode::FileType};
use std::{fs::File, io, os::unix::fs::FileExt, sync::Arc};

struct Image {
    file: File,
    size: u64,
}

impl BlockRead for Image {
    fn size_bytes(&self) -> u64 {
        self.size
    }
    fn read_at(&self, offset: u64, out: &mut [u8]) -> Result<(), Error> {
        self.file.read_exact_at(out, offset).map_err(|_| Error)
    }
}

impl BlockDevice for Image {
    fn is_writable(&self) -> bool {
        true
    }
    fn write_at(&self, offset: u64, data: &[u8]) -> Result<(), Error> {
        if !offset
            .checked_add(data.len() as u64)
            .is_some_and(|end| end <= self.size)
        {
            return Err(Error);
        }
        self.file.write_all_at(data, offset).map_err(|_| Error)
    }
    fn flush(&self) -> Result<(), Error> {
        self.file.sync_all().map_err(|_| Error)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("expected a fresh Btrfs image")?;
    let file = File::options().read(true).write(true).open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err("expected a regular image file".into());
    }
    let image = Arc::new(Image {
        file,
        size: metadata.len(),
    });
    let mut fs = Filesystem::mount_rw(image).map_err(io::Error::other)?;
    let root = fs.root_inode().map_err(io::Error::other)?;
    let dev = fs.lookup(root.ino, b"dev").map_err(io::Error::other)?;
    if dev.file_type() != Some(FileType::Directory) {
        return Err("/dev must be a directory in the new root".into());
    }
    // Linux encoding of the null device, major 1 and minor 3.
    fs.create_character_device(dev.ino, b"null", 0o666, 0x103)
        .map_err(io::Error::other)?;
    Ok(())
}
