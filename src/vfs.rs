//! Native pathname and open-file interface. Callers supply their own directory context.
#![forbid(unsafe_code)]
pub(crate) use crate::fs::Directories;
use crate::fs::{self as disk, FileError, OpenFile};
use dosrs_native_posix::{Errno, Open, Result};

pub(crate) struct File(OpenFile);
impl File {
    pub fn read(&self, offset: usize, out: &mut [u8]) -> Result<usize> {
        self.0.read(offset, out).map_err(file_error)
    }
    pub fn write(&self, offset: usize, data: &[u8]) -> Result<()> {
        self.0.write(offset, data).map_err(file_error)
    }
    pub fn sync(&self) -> Result<()> {
        if self.0.sync() {
            Ok(())
        } else {
            Err(Errno::Io)
        }
    }
}
fn file_error(error: FileError) -> Errno {
    match error {
        FileError::Missing => Errno::NoEntry,
        FileError::Directory => Errno::IsDirectory,
        FileError::Access => Errno::Access,
        FileError::Limit => Errno::TooManyFiles,
        FileError::Space => Errno::NoSpace,
        FileError::Io => Errno::Io,
    }
}

// Reject names the DOS short-name parser would otherwise silently truncate.
fn path_name(cwd: Directories, path: &str) -> Result<(u32, [u8; 11])> {
    let tail = if path.as_bytes().get(1) == Some(&b':') {
        &path[2..]
    } else {
        path
    };
    let mut parts = tail.split(['/', '\\']).peekable();
    while let Some(part) = parts.next() {
        if part.is_empty() || matches!(part, "." | "..") {
            if parts.peek().is_none() {
                return Err(Errno::Invalid);
            }
            continue;
        }
        let (base, ext) = part.split_once('.').unwrap_or((part, ""));
        if base.len() > 8 || ext.len() > 3 {
            return Err(Errno::NameTooLong);
        }
        if base.is_empty()
            || part.ends_with('.')
            || ext.contains('.')
            || !part
                .bytes()
                .all(|b| b.is_ascii_graphic() && !b"\"*+,:;<=>?[\\]|".contains(&b))
        {
            return Err(Errno::Invalid);
        }
    }
    let (base, tail) = cwd.locate(path.as_bytes()).ok_or(Errno::NoEntry)?;
    disk::file_in(base, tail).ok_or(Errno::NoEntry)
}

pub(crate) fn open(cwd: Directories, path: &str, flags: Open) -> Result<(File, bool)> {
    let (dir, name) = path_name(cwd, path)?;
    if flags.truncate && !flags.access.writable() {
        return Err(Errno::Invalid);
    }
    if flags.create && flags.exclusive && disk::exists_at(dir, &name) {
        return Err(Errno::Exists);
    }
    let mut created = false;
    let object = match OpenFile::open(dir, &name, flags.access.writable()) {
        Ok(object) => object,
        Err(FileError::Missing) if flags.create => {
            if !disk::write_named_at(dir, &name, &[], 0x20) {
                return Err(Errno::NoSpace);
            }
            match OpenFile::open(dir, &name, flags.access.writable()) {
                Ok(object) => {
                    created = true;
                    object
                }
                Err(err) => {
                    disk::remove_file_at(dir, &name);
                    return Err(file_error(err));
                }
            }
        }
        Err(e) => return Err(file_error(e)),
    };
    if flags.truncate && !created {
        object.write(0, &[]).map_err(file_error)?;
    }
    Ok((File(object), created || flags.truncate))
}
pub(crate) fn unlink(cwd: Directories, path: &str) -> Result<()> {
    let (dir, name) = path_name(cwd, path)?;
    let attr = disk::attribute_at(dir, &name).ok_or(Errno::NoEntry)?;
    if attr & 0x10 != 0 {
        return Err(Errno::IsDirectory);
    }
    if attr & 1 != 0 {
        return Err(Errno::Access);
    }
    if disk::remove_file_at(dir, &name) && disk::flush(dir) {
        Ok(())
    } else {
        Err(Errno::Io)
    }
}
