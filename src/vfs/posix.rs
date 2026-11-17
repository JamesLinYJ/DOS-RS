//! Native process directory and credentials, separate from the DOS drive context.
use super::{Directories, File, btrfs};
use alloc::vec::Vec;
use dosrs_native_posix::{Errno, Open, Result};

#[derive(Clone, Copy)]
pub(crate) struct PosixDir(pub u64);
pub(crate) const ROOT: PosixDir = PosixDir(256);
#[derive(Clone, Copy)]
pub(crate) struct Credentials {
    pub uid: u32,
    pub gid: u32,
}
pub(crate) const ADMIN: Credentials = Credentials { uid: 0, gid: 0 };
#[derive(Clone, Copy)]
pub(crate) struct Times {
    pub value: [Option<(i64, u32)>; 2],
    pub both_now: bool,
}

pub(crate) enum Attribute {
    Access(u32),
    Mode(u32),
    Owner(u32, u32),
    Times(Times),
}
pub(crate) use btrfs::posix::{
    capacity, change_at, chdir, check_access, chmod, chown, getcwd, link, link_at, mkdir,
    pin_directory, readlink, rename, rename_at, rmdir, stat, symlink, unlink,
};
pub(crate) fn native(path: &[u8]) -> bool {
    btrfs::is_mounted()
        && super::dos::mount_path(path).is_none()
        && !(path.len() >= 2 && path[0].is_ascii_alphabetic() && path[1] == b':')
}
pub(crate) fn initial(dos: Directories) -> PosixDir {
    let Some((dir, _)) = dos.locate(b"") else {
        return ROOT;
    };
    #[cfg(not(test))]
    if super::driver::btrfs_drive() && dir & super::driver::HARD != 0 {
        return btrfs::posix::from_dos(dir).unwrap_or(ROOT);
    }
    let _ = dir;
    ROOT
}
pub(crate) fn open(
    dos: Directories,
    cwd: PosixDir,
    path: &[u8],
    flags: Open,
    mode: u32,
    credentials: Credentials,
    executable: bool,
) -> Result<(File, bool)> {
    if native(path) {
        let (object, dirty) = btrfs::posix::open(cwd, path, flags, mode, credentials, executable)?;
        return Ok((alloc::boxed::Box::new(object), dirty));
    }
    super::open_dos(
        dos,
        core::str::from_utf8(path).map_err(|_| Errno::Invalid)?,
        flags,
    )
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Metadata {
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub blocks: u64,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub times: [(i64, u32); 3],
}
impl Metadata {
    pub(crate) fn bytes(self) -> [u8; 96] {
        let mut out = [0; 96];
        for (offset, word) in [self.dev, self.ino, self.size, self.blocks]
            .into_iter()
            .enumerate()
        {
            out[offset * 8..offset * 8 + 8].copy_from_slice(&word.to_le_bytes());
        }
        for (offset, word) in [self.mode, self.nlink, self.uid, self.gid]
            .into_iter()
            .enumerate()
        {
            let i = 32 + offset * 4;
            out[i..i + 4].copy_from_slice(&word.to_le_bytes());
        }
        for (i, (sec, nsec)) in self.times.into_iter().enumerate() {
            let i = 48 + i * 16;
            out[i..i + 8].copy_from_slice(&sec.to_le_bytes());
            out[i + 8..i + 12].copy_from_slice(&nsec.to_le_bytes());
        }
        out
    }
}
pub(crate) struct DirEntry {
    pub ino: u64,
    pub kind: u8,
    pub name: Vec<u8>,
}
