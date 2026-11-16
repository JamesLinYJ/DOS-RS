//! Read NTFS 3.x on-disk structures directly through the kernel block interface.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;
mod index;
mod record;

use alloc::{string::String, vec, vec::Vec};
use dosrs_block::BlockRead;
pub use index::Entry;
use record::*;
pub const ROOT: u64 = 5;
const RECORD_MASK: u64 = (1 << 48) - 1;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Io,
    Invalid,
    Missing,
    Unsupported,
    Directory,
}
pub type Result<T> = core::result::Result<T, Error>;
impl From<dosrs_block::Error> for Error {
    fn from(_: dosrs_block::Error) -> Self {
        Self::Io
    }
}

pub struct Filesystem<D> {
    dev: D,
    cluster: u64,
    capacity: u64,
    record_size: usize,
    mft: Stream,
    upcase: Vec<u8>,
    pub serial: u64,
    pub label: String,
}
pub struct File {
    stream: Stream,
    pub modified: i64,
    pub id: u64,
}
impl File {
    pub fn len(&self) -> u64 {
        self.stream.size()
    }
}
impl<D: BlockRead> Filesystem<D> {
    pub fn mount(dev: D) -> Result<Self> {
        let mut boot = [0; 512];
        dev.read_at(0, &mut boot)?;
        if &boot[3..11] != b"NTFS    " || boot[510..] != [0x55, 0xaa] {
            return Err(Error::Invalid);
        }
        if word(&boot, 11)? != 512 {
            return Err(Error::Unsupported);
        }
        let spc = u64::from(boot[13]);
        if !spc.is_power_of_two() || spc > 128 {
            return Err(Error::Unsupported);
        }
        let cluster = 512 * spc;
        let capacity = quad(&boot, 40)?
            .checked_mul(512)
            .filter(|n| *n <= dev.size_bytes())
            .ok_or(Error::Invalid)?;
        let units = boot[64] as i8;
        let size = if units < 0 {
            1u64.checked_shl(u32::from(units.unsigned_abs()))
        } else {
            (units as u64).checked_mul(cluster)
        }
        .ok_or(Error::Invalid)?;
        if !(512..=65536).contains(&size) || !size.is_power_of_two() {
            return Err(Error::Unsupported);
        }
        let addr = quad(&boot, 48)?
            .checked_mul(cluster)
            .ok_or(Error::Invalid)?;
        if addr.checked_add(size).is_none_or(|end| end > capacity) {
            return Err(Error::Invalid);
        }
        let mut rec = vec![0; size as usize];
        dev.read_at(addr, &mut rec)?;
        fixup(&mut rec, b"FILE")?;
        let mft = Stream::parse(attr(&rec, 0x80, "")?, cluster, capacity)?;
        let mut fs = Self {
            dev,
            cluster,
            capacity,
            record_size: size as usize,
            mft,
            upcase: Vec::new(),
            serial: quad(&boot, 72)?,
            label: String::new(),
        };
        let volume = fs.record(3)?;
        let info = resident(attr(&volume, 0x70, "")?)?;
        if info.get(8) != Some(&3) {
            return Err(Error::Unsupported);
        }
        if let Ok(a) = attr(&volume, 0x60, "") {
            fs.label = utf16(resident(a)?)?
        }
        let upper = fs.record(10)?;
        fs.upcase = fs.stream(&upper, 0x80, "")?.entire(&fs.dev, 131072)?;
        if fs.upcase.len() != 131072 {
            return Err(Error::Unsupported);
        }
        fs.record(ROOT)?;
        Ok(fs)
    }
    fn stream(&self, r: &[u8], ty: u32, name: &str) -> Result<Stream> {
        Stream::parse(attr(r, ty, name)?, self.cluster, self.capacity)
    }
    fn record(&self, id: u64) -> Result<Vec<u8>> {
        let mut r = vec![0; self.record_size];
        let pos = (id & RECORD_MASK)
            .checked_mul(self.record_size as u64)
            .ok_or(Error::Invalid)?;
        if self.mft.read(&self.dev, pos, &mut r)? != r.len() {
            return Err(Error::Missing);
        }
        fixup(&mut r, b"FILE")?;
        if word(&r, 22)? & 1 == 0 || (id >> 48 != 0 && u64::from(word(&r, 16)?) != id >> 48) {
            return Err(Error::Missing);
        }
        if quad(&r, 32)? != 0 {
            return Err(Error::Invalid);
        }
        Ok(r)
    }
    pub fn open(&self, id: u64) -> Result<File> {
        let r = self.record(id)?;
        if word(&r, 22)? & 2 != 0 {
            return Err(Error::Directory);
        }
        let meta = resident(attr(&r, 0x10, "")?)?;
        if dword(meta, 32)? & (0x400 | 0x800 | 0x4000) != 0 {
            return Err(Error::Unsupported);
        }
        Ok(File {
            id,
            modified: unix_time(quad(meta, 8)?),
            stream: self.stream(&r, 0x80, "")?,
        })
    }
    pub fn read(&self, file: &File, pos: u64, out: &mut [u8]) -> Result<usize> {
        file.stream.read(&self.dev, pos, out)
    }
    pub fn lookup(&self, parent: u64, name: &str) -> Result<Entry> {
        let folded: Vec<_> = name.encode_utf16().map(|c| self.fold(c)).collect();
        self.visit(parent, |entry| {
            entry
                .name
                .encode_utf16()
                .map(|c| self.fold(c))
                .eq(folded.iter().copied())
        })?
        .ok_or(Error::Missing)
    }
    fn fold(&self, c: u16) -> u16 {
        let i = usize::from(c) * 2;
        u16::from_le_bytes([self.upcase[i], self.upcase[i + 1]])
    }
}
fn unix_time(windows: u64) -> i64 {
    (windows / 10_000_000) as i64 - 11_644_473_600
}
fn utf16(bytes: &[u8]) -> Result<String> {
    if bytes.len() % 2 != 0 {
        return Err(Error::Invalid);
    }
    let chars = bytes
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]));
    core::char::decode_utf16(chars)
        .collect::<core::result::Result<String, _>>()
        .map_err(|_| Error::Unsupported)
}

impl<D: BlockRead> Filesystem<D> {
    pub fn space(&self) -> Result<(u32, u32, u32, u32)> {
        let total = self.capacity / self.cluster;
        let bitmap = self.stream(&self.record(6)?, 0x80, "")?;
        let mut block = [0; 4096];
        let mut used = 0u64;
        let bytes = total.div_ceil(8);
        let mut pos = 0;
        while pos < bytes {
            let n = (bytes - pos).min(block.len() as u64) as usize;
            if bitmap.read(&self.dev, pos, &mut block[..n])? != n {
                return Err(Error::Invalid);
            }
            if pos + n as u64 == bytes && total % 8 != 0 {
                block[n - 1] &= (1 << (total % 8)) - 1
            }
            used += block[..n]
                .iter()
                .map(|b| u64::from(b.count_ones()))
                .sum::<u64>();
            pos += n as u64;
        }
        Ok((
            (self.cluster / 512) as u32,
            512,
            (total - used).min(u64::from(u32::MAX)) as u32,
            total.min(u64::from(u32::MAX)) as u32,
        ))
    }
}
