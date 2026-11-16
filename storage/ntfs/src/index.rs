//! Traverse the $I30 directory tree. Only follow live, referenced index blocks.
use super::*;
use alloc::collections::BTreeSet;

pub struct Entry {
    pub name: String,
    pub id: u64,
    pub attrs: u32,
    pub size: u64,
    pub modified: i64,
}
impl Entry {
    pub fn directory(&self) -> bool {
        self.attrs & (0x10 | 0x1000_0000) != 0
    }
    pub fn reparse(&self) -> bool {
        self.attrs & 0x400 != 0
    }
}
impl<D: BlockRead> Filesystem<D> {
    pub fn visit(
        &self,
        parent: u64,
        mut accept: impl FnMut(&Entry) -> bool,
    ) -> Result<Option<Entry>> {
        let rec = self.record(parent)?;
        if word(&rec, 22)? & 2 == 0 {
            return Err(Error::Directory);
        }
        let root = resident(attr(&rec, 0x90, "$I30")?)?;
        if dword(root, 0)? != 0x30 || dword(root, 4)? != 1 {
            return Err(Error::Unsupported);
        }
        let size = dword(root, 8)? as usize;
        if !(512..=65536).contains(&size) || !size.is_power_of_two() {
            return Err(Error::Invalid);
        }
        let mut pending = Vec::new();
        let mut seen = BTreeSet::new();
        let found = scan_node(root, 16, parent, &mut pending, &mut accept)?;
        if found.is_some() || pending.is_empty() {
            return Ok(found);
        }
        let nodes = self.stream(&rec, 0xa0, "$I30")?;
        let bitmap = self.stream(&rec, 0xb0, "$I30")?;
        let unit = if self.cluster > size as u64 {
            512
        } else {
            self.cluster
        };
        let mut block = vec![0; size];
        while let Some(vcn) = pending.pop() {
            if seen.len() >= 65536 {
                return Err(Error::Unsupported);
            }
            if !seen.insert(vcn) {
                return Err(Error::Invalid);
            }
            let pos = vcn.checked_mul(unit).ok_or(Error::Invalid)?;
            if pos % size as u64 != 0 {
                return Err(Error::Invalid);
            }
            let bit = pos / size as u64;
            let mut byte = [0];
            if bitmap.read(&self.dev, bit / 8, &mut byte)? != 1 || byte[0] & (1 << (bit % 8)) == 0 {
                return Err(Error::Invalid);
            }
            if nodes.read(&self.dev, pos, &mut block)? != size {
                return Err(Error::Invalid);
            }
            fixup(&mut block, b"INDX")?;
            if quad(&block, 16)? != vcn {
                return Err(Error::Invalid);
            }
            if let Some(e) = scan_node(&block, 24, parent, &mut pending, &mut accept)? {
                return Ok(Some(e));
            }
        }
        Ok(None)
    }
}
fn scan_node(
    b: &[u8],
    header: usize,
    parent: u64,
    pending: &mut Vec<u64>,
    accept: &mut impl FnMut(&Entry) -> bool,
) -> Result<Option<Entry>> {
    let h = b.get(header..).ok_or(Error::Invalid)?;
    let mut at = dword(h, 0)? as usize;
    let used = dword(h, 4)? as usize;
    if at < 16 || at > used {
        return Err(Error::Invalid);
    }
    let node = slice(h, 0, used)?;
    loop {
        let size = usize::from(word(node, at + 8)?);
        let key_len = usize::from(word(node, at + 10)?);
        let flags = word(node, at + 12)?;
        if size < 16 || size % 8 != 0 || flags & !3 != 0 {
            return Err(Error::Invalid);
        }
        let entry = slice(node, at, size)?;
        let child = flags & 1 != 0;
        if child {
            if size < 24 || pending.len() >= 65536 {
                return Err(Error::Invalid);
            }
            pending.push(quad(entry, size - 8)?);
        }
        if flags & 2 != 0 {
            return Ok(None);
        }
        let key = slice(&entry[..size - usize::from(child) * 8], 16, key_len)?;
        let len = usize::from(*key.get(64).ok_or(Error::Invalid)?);
        let id = quad(entry, 0)?;
        if (quad(key, 0)? & RECORD_MASK) != (parent & RECORD_MASK) {
            return Err(Error::Invalid);
        }
        if id & RECORD_MASK >= 16 {
            let name = slice(key, 66, len * 2)?;
            // Names with unpaired UTF-16 surrogates cannot be addressed through a UTF-8 pathname.
            if let Ok(name) = utf16(name) {
                let e = Entry {
                    id,
                    name,
                    attrs: dword(key, 56)?,
                    size: quad(key, 48)?,
                    modified: unix_time(quad(key, 16)?),
                };
                if accept(&e) {
                    return Ok(Some(e));
                }
            }
        }
        at += size;
    }
}
