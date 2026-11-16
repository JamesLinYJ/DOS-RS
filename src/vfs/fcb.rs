//! DOS file control blocks. Native and interpreted callers marshal the same 37 bytes.
use super::dos::{self, OpenFile};
use alloc::{collections::BTreeMap, vec};
pub(crate) type Record = [u8; 37];
pub(crate) fn name(raw: &Record) -> &[u8; 11] {
    raw[1..12].try_into().unwrap()
}
pub(crate) fn location(raw: &Record) -> Option<(u8, u32)> {
    let drive = raw[0].checked_sub(1).unwrap_or_else(dos::active_drive);
    Some((drive + 1, dos::drive_directory(drive)?))
}
fn word(raw: &Record, at: usize) -> u16 {
    u16::from_le_bytes(raw[at..at + 2].try_into().unwrap())
}
fn number(raw: &Record, at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}
fn record_size(raw: &Record) -> usize {
    match word(raw, 14) {
        0 => 128,
        n => usize::from(n),
    }
}
fn sequential(raw: &Record) -> u32 {
    u32::from(word(raw, 12)) * 128 + u32::from(raw[32])
}
fn set_sequential(raw: &mut Record, n: u32) {
    raw[12..14].copy_from_slice(&((n / 128) as u16).to_le_bytes());
    raw[32] = (n % 128) as u8;
}
fn random(raw: &Record) -> u32 {
    number(raw, 33)
        & if record_size(raw) < 64 {
            u32::MAX
        } else {
            0xffffff
        }
}
fn set_random(raw: &mut Record, n: u32) {
    let width = if record_size(raw) < 64 { 4 } else { 3 };
    raw[33..33 + width].copy_from_slice(&n.to_le_bytes()[..width]);
}
pub(crate) fn place(raw: &mut Record) {
    set_random(raw, sequential(raw))
}
pub(crate) fn size(raw: &mut Record) -> Option<()> {
    let (_, dir) = location(raw)?;
    let f = OpenFile::open(dir, name(raw), false).ok()?;
    set_random(raw, f.len().div_ceil(record_size(raw)) as u32);
    Some(())
}
struct Opened {
    file: OpenFile,
    owner: u16,
    writable: bool,
    size: u32,
    stamp: (u16, u16),
}
pub(crate) struct Table {
    next: u32,
    entries: BTreeMap<u32, Opened>,
}
impl Table {
    pub const fn new() -> Self {
        Self {
            next: 1,
            entries: BTreeMap::new(),
        }
    }
    pub fn release(&mut self, owner: u16) {
        self.entries.retain(|_, e| {
            if e.owner != owner {
                return true;
            }
            let _ = e.file.sync();
            false
        })
    }
    pub fn open(&mut self, owner: u16, raw: &mut Record, create: bool) -> Option<()> {
        let (drive, dir) = location(raw)?;
        if self.entries.len() >= 256 {
            return None;
        }
        let id = self.next;
        self.next = self.next.checked_add(1)?;
        if create && !dos::write_named_at(dir, name(raw), &[], 0x20) {
            return None;
        }
        let (file, writable) = match OpenFile::open(dir, name(raw), true) {
            Ok(file) => (file, true),
            Err(_) => (OpenFile::open(dir, name(raw), false).ok()?, false),
        };
        raw[0] = drive;
        set_sequential(raw, 0);
        raw[14..16].copy_from_slice(&128u16.to_le_bytes());
        let size = file.len() as u32;
        raw[16..20].copy_from_slice(&size.to_le_bytes());
        let stamp = file.stamp();
        raw[20..22].copy_from_slice(&stamp.1.to_le_bytes());
        raw[22..24].copy_from_slice(&stamp.0.to_le_bytes());
        raw[24..28].copy_from_slice(&id.to_le_bytes());
        self.entries.insert(
            id,
            Opened {
                file,
                owner,
                writable,
                size,
                stamp,
            },
        );
        Some(())
    }
    fn get(&mut self, owner: u16, raw: &Record) -> Option<&mut Opened> {
        self.entries
            .get_mut(&number(raw, 24))
            .filter(|e| e.owner == owner)
    }
    pub fn close(&mut self, owner: u16, raw: &mut Record) -> Option<()> {
        let size = number(raw, 16);
        let stamp = (word(raw, 22), word(raw, 20));
        let e = self.get(owner, raw)?;
        if size != e.size && (!e.writable || e.file.write(size as usize, &[]).is_err()) {
            return None;
        }
        if stamp != e.stamp && (!e.writable || !e.file.set_stamp(stamp.0, stamp.1)) {
            return None;
        }
        if !e.file.sync() {
            return None;
        }
        self.entries.remove(&number(raw, 24));
        raw[24..28].fill(0);
        Some(())
    }

    /// The transfer cannot cross the end of the caller's DTA segment.
    pub fn transfer(
        &mut self,
        owner: u16,
        raw: &mut Record,
        ah: u8,
        requested: u16,
        room: usize,
        mut copy: impl FnMut(usize, &mut [u8], bool) -> bool,
    ) -> (u8, u16) {
        let write = matches!(ah, 0x15 | 0x22 | 0x28);
        let block = matches!(ah, 0x27 | 0x28);
        let seq = matches!(ah, 0x14 | 0x15);
        let mut index = if seq { sequential(raw) } else { random(raw) };
        let bytes = record_size(raw);
        let Some(e) = self.get(owner, raw) else {
            return (0xff, 0);
        };
        if write && !e.writable {
            return (1, 0);
        }
        if block && write && requested == 0 {
            let Some(at) = (index as usize)
                .checked_mul(bytes)
                .filter(|n| *n <= u32::MAX as usize)
            else {
                return (1, 0);
            };
            if e.file.write(at, &[]).is_err() {
                return (1, 0);
            }
            e.size = at as u32;
            raw[16..20].copy_from_slice(&e.size.to_le_bytes());
            return (0, 0);
        }
        let count = if block { requested } else { 1 };
        let mut record = vec![0; bytes];
        let mut done = 0u16;
        let mut status = 0;
        while done < count {
            let dta = usize::from(done) * bytes;
            if dta + bytes > room {
                status = 2;
                break;
            }
            let Some(at) = (index as usize).checked_mul(bytes) else {
                status = 1;
                break;
            };
            if write {
                if (at as u64) + (bytes as u64) > u64::from(u32::MAX) {
                    status = 1;
                    break;
                }
                if !copy(dta, &mut record, true) {
                    status = 2;
                    break;
                }
                if e.file.write(at, &record).is_err() {
                    status = 1;
                    break;
                }
                e.size = e.file.len() as u32;
                raw[16..20].copy_from_slice(&e.size.to_le_bytes());
            } else {
                match e.file.read(at, &mut record) {
                    Ok(0) => {
                        status = 1;
                        break;
                    }
                    Ok(n) if n < bytes => {
                        record[n..].fill(0);
                        status = 3
                    }
                    Ok(_) => {}
                    Err(_) => {
                        status = 0xff;
                        break;
                    }
                }
                if !copy(dta, &mut record, false) {
                    status = 2;
                    break;
                }
            }
            index = index.saturating_add(1);
            done += 1;
            if status != 0 {
                break;
            }
        }
        if seq || block {
            set_sequential(raw, index)
        }
        if block {
            set_random(raw, index)
        }
        (status, done)
    }
}
pub(crate) fn delete(raw: &Record) -> Option<()> {
    let (_, dir) = location(raw)?;
    let mut removed = false;
    // After each removal the backend may compact directory entries.
    while let Some(entry) = dos::find_match(dir, name(raw), 0, 0) {
        if !dos::in_dir(dir, || dos::remove_file(&entry.name)) {
            break;
        }
        removed = true;
    }
    removed.then_some(())
}
pub(crate) fn rename(raw: &Record) -> Option<()> {
    let (_, dir) = location(raw)?;
    let to = raw[18..29].try_into().ok()?;
    dos::in_dir(dir, || dos::rename_file(name(raw), to)).then_some(())
}

/// Copy a record between the caller's DTA and a bounded transfer buffer.
pub(crate) fn service(
    table: &mut Table,
    owner: u16,
    raw: &mut Record,
    ah: u8,
    attr: u8,
    count: u16,
    room: usize,
    mut copy: impl FnMut(usize, &mut [u8], bool) -> bool,
) -> (u8, u16) {
    let ok = match ah {
        0x0f | 0x16 => table.open(owner, raw, ah == 0x16),
        0x10 => table.close(owner, raw),
        0x11 | 0x12 => {
            let found = location(raw).and_then(|(drive, dir)| {
                let start = if ah == 0x12 { word(raw, 24) } else { 0 };
                let f = dos::find_match(dir, name(raw), attr, start)?;
                let mut data = [0; 33];
                data[0] = drive;
                data[1..12].copy_from_slice(&f.name);
                data[14] = 128;
                data[16..20].copy_from_slice(&f.size.to_le_bytes());
                data[20..22].copy_from_slice(&f.date.to_le_bytes());
                data[22..24].copy_from_slice(&f.time.to_le_bytes());
                if room < data.len() || !copy(0, &mut data, false) {
                    return None;
                }
                raw[24..26].copy_from_slice(&f.index.saturating_add(1).to_le_bytes());
                Some(())
            });
            found
        }
        0x13 => delete(raw),
        0x17 => rename(raw),
        0x14 | 0x15 | 0x21 | 0x22 | 0x27 | 0x28 => {
            return table.transfer(owner, raw, ah, count, room, copy);
        }
        0x23 => size(raw),
        0x24 => {
            place(raw);
            Some(())
        }
        _ => None,
    };
    (if ok.is_some() { 0 } else { 0xff }, count)
}
