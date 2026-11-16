//! A DOS directory token stays associated with the same mounted directory for its lifetime.
use super::driver::HARD;
use alloc::{vec, vec::Vec};
use dosrs_native_posix::{Errno, Result};

pub(super) struct Folder {
    pub id: u64,
    pub parent: u32,
    name: Vec<u8>,
}
pub(super) struct Folders(Vec<Option<Folder>>);
impl Folders {
    pub fn new(root: u64) -> Self {
        Self(vec![Some(Folder {
            id: root,
            parent: HARD,
            name: Vec::new(),
        })])
    }
    pub fn get(&self, token: u32) -> Result<&Folder> {
        self.0
            .get((token & !HARD) as usize)
            .and_then(Option::as_ref)
            .ok_or(Errno::NoEntry)
    }
    pub fn remember(&mut self, id: u64, parent: u32, name: Vec<u8>) -> Result<u32> {
        if let Some(i) = self
            .0
            .iter()
            .position(|f| f.as_ref().is_some_and(|f| f.id == id))
        {
            return Ok(HARD | i as u32);
        }
        if self.0.len() >= 4096 {
            return Err(Errno::NoSpace);
        }
        let token = HARD | self.0.len() as u32;
        self.0.push(Some(Folder { id, parent, name }));
        Ok(token)
    }
    pub fn remove(&mut self, id: u64) {
        if let Some(entry) = self
            .0
            .iter_mut()
            .find(|f| f.as_ref().is_some_and(|f| f.id == id))
        {
            *entry = None
        }
    }
    pub fn rename(&mut self, id: u64, new: Vec<u8>) {
        if let Some(entry) = self.0.iter_mut().flatten().find(|f| f.id == id) {
            entry.name = new
        }
    }
    pub fn path(&self, mut token: u32, out: &mut [u8]) -> Result<usize> {
        let mut parts = Vec::new();
        while token != HARD {
            let folder = self.get(token)?;
            parts.push(&folder.name);
            token = folder.parent;
        }
        let mut used = 0;
        for name in parts.into_iter().rev() {
            if used > 0 && used < out.len() {
                out[used] = b'\\';
                used += 1
            }
            let n = (out.len() - used).min(name.len());
            out[used..used + n].copy_from_slice(&name[..n]);
            used += n;
        }
        Ok(used)
    }
}
