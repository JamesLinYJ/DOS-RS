//! Open FAT objects are identified by volume and directory slot. Unlink detaches
//! the FAT metadata before its slot can be reused. The final close frees its chain.
use super::{Found, disk, find, flush, layout, read_chain, root, write_chain};
use crate::kernel_cell::KernelCell;

use super::FileError;

#[derive(Clone, Copy)]
struct Pin {
    volume: u32,
    slot: usize,
    opens: usize,
    detached: Option<[u8; 32]>,
}
static PINS: KernelCell<[Option<Pin>; 256]> = KernelCell::new([None; 256]);

pub struct OpenFile(usize);

impl OpenFile {
    pub fn open(dir: u32, name: &[u8; 11], writable: bool) -> Result<Self, FileError> {
        let disk = disk(dir);
        let map = layout(&disk).ok_or(FileError::Io)?;
        let f = find(&disk, &map, dir, name).ok_or(FileError::Missing)?;
        if f.attr & 0x10 != 0 {
            return Err(FileError::Directory);
        }
        if writable && f.attr & 1 != 0 {
            return Err(FileError::Access);
        }
        let (clusters, _) = map.chain(&disk.bytes, f.cluster).ok_or(FileError::Io)?;
        if f.size.div_ceil(map.cluster_bytes()) > clusters {
            return Err(FileError::Io);
        }
        let volume = root(dir);
        let mut pins = PINS.borrow();
        for (id, entry) in pins.iter_mut().enumerate() {
            if let Some(pin) = entry
                && pin.linked(volume, f.at)
            {
                pin.opens += 1;
                return Ok(Self(id));
            }
        }
        let id = pins
            .iter()
            .position(Option::is_none)
            .ok_or(FileError::Limit)?;
        pins[id] = Some(Pin {
            volume,
            slot: f.at,
            opens: 1,
            detached: None,
        });
        Ok(Self(id))
    }

    pub fn read(&self, offset: usize, out: &mut [u8]) -> Result<usize, FileError> {
        let pin = PINS.borrow()[self.0].unwrap();
        let disk = disk(pin.volume);
        let map = layout(&disk).ok_or(FileError::Io)?;
        read_chain(&disk, &map, &pin.record(&disk.bytes), offset, out).ok_or(FileError::Io)
    }

    /// An empty transfer sets the length, as needed by O_TRUNC.
    pub fn write(&self, offset: usize, bytes: &[u8]) -> Result<(), FileError> {
        let mut pins = PINS.borrow();
        let pin = pins[self.0].as_mut().unwrap();
        let mut disk = disk(pin.volume);
        let map = layout(&disk).ok_or(FileError::Io)?;
        let mut raw = pin.record(&disk.bytes);
        if raw[11] & 1 != 0 {
            return Err(FileError::Access);
        }
        if !write_chain(&mut disk, &map, &mut raw, offset, bytes) {
            return Err(FileError::Space);
        }
        pin.update(&mut disk, raw);
        Ok(())
    }

    pub fn volume(&self) -> u32 {
        PINS.borrow()[self.0].unwrap().volume
    }
    pub fn identity(&self) -> usize {
        self.0
    }

    pub fn len(&self) -> usize {
        let pin = PINS.borrow()[self.0].unwrap();
        super::dword(&pin.record(&disk(pin.volume).bytes), 28) as usize
    }

    pub fn stamp(&self) -> (u16, u16) {
        let pin = PINS.borrow()[self.0].unwrap();
        let raw = pin.record(&disk(pin.volume).bytes);
        (super::word(&raw, 22), super::word(&raw, 24))
    }

    pub fn set_stamp(&self, time: u16, date: u16) {
        let mut pins = PINS.borrow();
        let pin = pins[self.0].as_mut().unwrap();
        let mut disk = disk(pin.volume);
        let mut raw = pin.record(&disk.bytes);
        raw[22..24].copy_from_slice(&time.to_le_bytes());
        raw[24..26].copy_from_slice(&date.to_le_bytes());
        pin.update(&mut disk, raw);
    }

    pub fn sync(&self) -> bool {
        flush(PINS.borrow()[self.0].unwrap().volume)
    }
}

impl Pin {
    fn update(&mut self, disk: &mut super::Disk, raw: [u8; 32]) {
        if self.detached.is_some() {
            self.detached = Some(raw);
        } else {
            disk.bytes
                .edit(self.slot..self.slot + 32)
                .copy_from_slice(&raw);
        }
    }
    fn linked(&self, volume: u32, slot: usize) -> bool {
        self.volume == volume && self.slot == slot && self.detached.is_none()
    }
    fn record(&self, bytes: &[u8]) -> [u8; 32] {
        self.detached
            .unwrap_or_else(|| bytes[self.slot..self.slot + 32].try_into().unwrap())
    }
}

pub(super) fn detach(dir: u32, file: &Found) -> bool {
    let mut pins = PINS.borrow();
    if let Some(pin) = pins
        .iter_mut()
        .flatten()
        .find(|p| p.linked(root(dir), file.at))
    {
        pin.detached = Some(file.raw);
        return true;
    }
    false
}

impl Drop for OpenFile {
    fn drop(&mut self) {
        let mut pins = PINS.borrow();
        let pin = pins[self.0].as_mut().unwrap();
        pin.opens -= 1;
        if pin.opens == 0 {
            let pin = pins[self.0].take().unwrap();
            if let Some(raw) = pin.detached {
                let mut disk = disk(pin.volume);
                if let Some(map) = layout(&disk) {
                    map.release(&mut disk.bytes, map.first(&raw));
                    map.refresh_fsinfo(&mut disk.bytes);
                }
                drop(disk);
                flush(pin.volume);
            }
        }
    }
}
