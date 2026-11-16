//! NTFS 3.x file records, update sequence arrays and attribute streams.
use super::{Error, Result};
use alloc::{vec, vec::Vec};
use dosrs_block::BlockRead;

pub(super) fn slice(b: &[u8], at: usize, size: usize) -> Result<&[u8]> {
    b.get(at..at.checked_add(size).ok_or(Error::Invalid)?)
        .ok_or(Error::Invalid)
}
pub(super) fn word(b: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(slice(b, at, 2)?.try_into().unwrap()))
}
pub(super) fn dword(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(slice(b, at, 4)?.try_into().unwrap()))
}
pub(super) fn quad(b: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(slice(b, at, 8)?.try_into().unwrap()))
}
pub(super) fn fixup(b: &mut [u8], magic: &[u8; 4]) -> Result<()> {
    if b.get(..4) != Some(magic) || !b.len().is_multiple_of(512) {
        return Err(Error::Invalid);
    }
    let at = usize::from(word(b, 4)?);
    let count = usize::from(word(b, 6)?);
    if at < 8 || at % 2 != 0 || count != 1 + b.len() / 512 || at + 2 * count > 510 {
        return Err(Error::Invalid);
    }
    let check = word(b, at)?;
    for i in 1..count {
        let end = i * 512;
        if word(b, end - 2)? != check {
            return Err(Error::Invalid);
        }
        let saved = word(b, at + 2 * i)?;
        b[end - 2..end].copy_from_slice(&saved.to_le_bytes());
    }
    Ok(())
}

pub(super) fn attr<'a>(record: &'a [u8], kind: u32, name: &str) -> Result<&'a [u8]> {
    let used = dword(record, 24)? as usize;
    let b = slice(record, 0, used)?;
    let mut pos = usize::from(word(b, 20)?);
    if pos < 42 || pos % 8 != 0 {
        return Err(Error::Invalid);
    }
    let mut extensions = false;
    loop {
        let ty = dword(b, pos)?;
        if ty == u32::MAX {
            return Err(if extensions {
                Error::Unsupported
            } else {
                Error::Missing
            });
        }
        extensions |= ty == 0x20;
        let size = dword(b, pos + 4)? as usize;
        if size < 24 || size % 8 != 0 {
            return Err(Error::Invalid);
        }
        let a = slice(b, pos, size)?;
        if ty == kind
            && usize::from(a[9]) == name.len()
            && (name.is_empty()
                || slice(a, usize::from(word(a, 10)?), 2 * name.len())?
                    .chunks_exact(2)
                    .map(|p| u16::from_le_bytes([p[0], p[1]]))
                    .eq(name.encode_utf16()))
        {
            return Ok(a);
        }
        pos += size;
    }
}

pub(super) fn resident(a: &[u8]) -> Result<&[u8]> {
    if a.get(8) != Some(&0) {
        return Err(Error::Unsupported);
    }
    let at = usize::from(word(a, 20)?);
    if at < 24 {
        return Err(Error::Invalid);
    }
    slice(a, at, dword(a, 16)? as usize)
}

pub(super) struct Run {
    end: u64,
    physical: Option<u64>,
}
pub(super) enum Stream {
    Inline(Vec<u8>),
    Runs {
        map: Vec<Run>,
        size: u64,
        initialized: u64,
    },
}
impl Stream {
    pub(super) fn parse(a: &[u8], cluster: u64, capacity: u64) -> Result<Self> {
        if word(a, 12)? & !0x8000 != 0 {
            return Err(Error::Unsupported);
        }
        if a[8] == 0 {
            return Ok(Self::Inline(resident(a)?.to_vec()));
        }
        if a[8] != 1 || quad(a, 16)? != 0 {
            return Err(Error::Unsupported);
        }
        let size = quad(a, 48)?;
        let initialized = quad(a, 56)?;
        let mut pos = usize::from(word(a, 32)?);
        if pos < 64 || initialized > size {
            return Err(Error::Invalid);
        }
        let mut map = Vec::new();
        let mut end = 0u64;
        let mut lcn = 0i64;
        loop {
            let tag = *a.get(pos).ok_or(Error::Invalid)?;
            pos += 1;
            if tag == 0 {
                break;
            }
            let len = usize::from(tag & 15);
            let off = usize::from(tag >> 4);
            if len == 0 || len > 8 || off > 8 {
                return Err(Error::Invalid);
            }
            let mut field = [0; 8];
            field[..len].copy_from_slice(slice(a, pos, len)?);
            let bytes = u64::from_le_bytes(field)
                .checked_mul(cluster)
                .filter(|n| *n != 0)
                .ok_or(Error::Invalid)?;
            pos += len;
            let physical = if off == 0 {
                None
            } else {
                let encoded = slice(a, pos, off)?;
                let mut delta = [if encoded[off - 1] & 128 != 0 { 255 } else { 0 }; 8];
                delta[..off].copy_from_slice(encoded);
                lcn = lcn
                    .checked_add(i64::from_le_bytes(delta))
                    .ok_or(Error::Invalid)?;
                let base = u64::try_from(lcn)
                    .ok()
                    .and_then(|n| n.checked_mul(cluster))
                    .ok_or(Error::Invalid)?;
                if base.checked_add(bytes).is_none_or(|n| n > capacity) {
                    return Err(Error::Invalid);
                }
                Some(base)
            };
            pos += off;
            end = end.checked_add(bytes).ok_or(Error::Invalid)?;
            map.push(Run { end, physical });
        }
        if end != 0
            && quad(a, 24)?
                .checked_add(1)
                .and_then(|c| c.checked_mul(cluster))
                != Some(end)
        {
            return Err(Error::Invalid);
        }
        if size > end {
            return Err(Error::Unsupported);
        } // Attribute list with further extents.
        Ok(Self::Runs {
            map,
            size,
            initialized,
        })
    }
    pub(super) fn size(&self) -> u64 {
        match self {
            Self::Inline(v) => v.len() as u64,
            Self::Runs { size, .. } => *size,
        }
    }
    pub(super) fn read(&self, dev: &dyn BlockRead, mut pos: u64, out: &mut [u8]) -> Result<usize> {
        let n = self.size().saturating_sub(pos).min(out.len() as u64) as usize;
        if n == 0 {
            return Ok(0);
        }
        match self {
            Self::Inline(bytes) => out[..n].copy_from_slice(&bytes[pos as usize..pos as usize + n]),
            Self::Runs {
                map, initialized, ..
            } => {
                let valid = initialized.saturating_sub(pos).min(n as u64) as usize;
                out[valid..n].fill(0);
                let mut done = 0;
                let mut i = map.partition_point(|r| r.end <= pos);
                while done < valid {
                    let run = map.get(i).ok_or(Error::Invalid)?;
                    let begin = if i == 0 { 0 } else { map[i - 1].end };
                    let step = (run.end - pos).min((valid - done) as u64) as usize;
                    let dest = &mut out[done..done + step];
                    if let Some(addr) = run.physical {
                        dev.read_at(addr + (pos - begin), dest)?
                    } else {
                        dest.fill(0)
                    }
                    done += step;
                    pos += step as u64;
                    i += 1;
                }
            }
        }
        Ok(n)
    }
    pub(super) fn entire(&self, dev: &dyn BlockRead, max: usize) -> Result<Vec<u8>> {
        if self.size() > max as u64 {
            return Err(Error::Unsupported);
        }
        let mut bytes = vec![0; self.size() as usize];
        self.read(dev, 0, &mut bytes)?;
        Ok(bytes)
    }
}
