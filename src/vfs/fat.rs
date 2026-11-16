//! Mounted FAT volumes shared by native processes, the DOS guest and BIOS sector reads.
#![forbid(unsafe_code)]
#[path = "fat/volume.rs"]
mod volume;
use volume::{Kind, Map, dword, word};

#[cfg(test)]
const IMAGE: &[u8] = include_bytes!("../../disks/guest12.img");
#[cfg(not(test))]
const IMAGE: &[u8] = include_bytes!(env!("DOS_RS_GUEST_IMAGE"));

#[path = "fat/media.rs"]
mod media;
#[path = "fat/open.rs"]
mod open;
use super::dos::FileError;
use super::dos::{DirMatch, DriveBlock, attribute_visible, names_match, root};
use super::driver::HARD;
#[cfg(not(test))]
pub(super) use media::mount;
use media::{Disk, disk};
pub(super) use media::{boot, flush};
pub use open::OpenFile;
fn layout(disk: &Disk) -> Option<Map> {
    Map::parse(&disk.bytes)
}
pub fn media_id(dir: u32) -> u8 {
    disk(dir).bytes[21]
}

pub fn drive_block(dir: u32) -> Option<DriveBlock> {
    let disk = disk(dir);
    let map = layout(&disk)?;
    // The DOS 4 DPB has no encoding for a FAT32 root.
    if map.kind == Kind::Fat32 {
        return None;
    }
    Some(DriveBlock {
        bytes_per_sector: map.bps as u16,
        sectors_per_cluster: map.spc as u8,
        reserved: word(&disk.bytes, 14),
        fats: disk.bytes[16],
        root_entries: word(&disk.bytes, 17),
        sectors_per_fat: map.fat_sectors as u16,
        media: disk.bytes[21],
        data_sector: map.data_sector.try_into().ok()?,
        max_cluster: map.max as u16,
        shift: map.spc.trailing_zeros() as u8,
    })
}

pub fn volume_id(dir: u32) -> Option<([u8; 4], [u8; 11], [u8; 8])> {
    let disk = disk(dir);
    let map = layout(&disk)?;
    let at = if map.kind == Kind::Fat32 { 66 } else { 38 };
    let mut serial = [0; 4];
    let mut label = *b"NO NAME    ";
    if disk.bytes[at] == 0x29 {
        serial.copy_from_slice(&disk.bytes[at + 1..at + 5]);
        label.copy_from_slice(&disk.bytes[at + 5..at + 16]);
    }
    Some((serial, label, map.kind.label()))
}

pub fn set_volume_id(dir: u32, serial: &[u8; 4], label: &[u8; 11]) -> bool {
    let disk = &mut disk(dir);
    let Some(map) = layout(disk) else {
        return false;
    };
    let at = if map.kind == Kind::Fat32 { 66 } else { 38 };
    for sector in [Some(0), map.backup].into_iter().flatten() {
        let p = sector * map.bps + at;
        disk.bytes.edit(p..p + 1)[0] = 0x29;
        disk.bytes.edit(p + 1..p + 5).copy_from_slice(serial);
        disk.bytes.edit(p + 5..p + 16).copy_from_slice(label);
        disk.bytes
            .edit(p + 16..p + 24)
            .copy_from_slice(&map.kind.label());
    }
    true
}

pub fn space(dir: u32) -> Option<(u32, u32, u32, u32)> {
    let disk = disk(dir);
    let map = layout(&disk)?;
    Some((
        map.spc as u32,
        map.bps as u32,
        map.free(&disk.bytes, 2).count() as u32,
        map.max - 1,
    ))
}

pub(crate) fn with_media<T>(read: impl FnOnce(&[u8]) -> T) -> T {
    read(&disk(0).bytes)
}

pub fn write_named_at(dir: u32, name: &[u8; 11], data: &[u8], attr: u8) -> bool {
    let disk = &mut disk(dir);
    let Some(map) = layout(disk) else {
        return false;
    };
    let previous = find(disk, &map, dir, name);
    if previous.as_ref().is_some_and(|f| f.attr & 0x10 != 0) {
        return false;
    }
    let slot = match &previous {
        Some(f) => Slot::At(f.at),
        None => match free_slot(disk, &map, dir) {
            Some(s) => s,
            None => return false,
        },
    };
    let old = previous.as_ref().map_or(0, |f| f.cluster);
    let Some((reusable, _)) = map.chain(&disk.bytes, old) else {
        return false;
    };
    let need = data.len().div_ceil(map.cluster_bytes()) + slot.extra();
    if !room(disk, &map, need.saturating_sub(reusable)) {
        return false;
    }
    map.release(&mut disk.bytes, old);
    let mut cursor = 2;
    let at = slot.claim(disk, &map, &mut cursor);
    let mut first = 0;
    let mut last = 0;
    for chunk in data.chunks(map.cluster_bytes()) {
        let cluster = map.allocate(&mut disk.bytes, &mut cursor);
        if first == 0 {
            first = cluster;
        } else {
            map.set(&mut disk.bytes, last, cluster);
        }
        let start = map.data(cluster).start;
        disk.bytes
            .edit(start..start + chunk.len())
            .copy_from_slice(chunk);
        last = cluster;
    }
    let mut raw = previous.map_or([0; 32], |f| f.raw);
    raw[..11].copy_from_slice(name);
    raw[11] = attr & 0x27;
    map.set_first(&mut raw, first);
    raw[28..32].copy_from_slice(&(data.len() as u32).to_le_bytes());
    disk.bytes.edit(at..at + 32).copy_from_slice(&raw);
    map.refresh_fsinfo(&mut disk.bytes);
    true
}

pub fn remove_file_at(dir: u32, name: &[u8; 11]) -> bool {
    remove(dir, name, false)
}
pub fn remove_dir(dir: u32, name: &[u8; 11]) -> bool {
    remove(dir, name, true)
}
fn remove(dir: u32, name: &[u8; 11], directory: bool) -> bool {
    let disk = &mut disk(dir);
    let Some(map) = layout(disk) else {
        return false;
    };
    let Some(f) = find(disk, &map, disk.cwd, name) else {
        return false;
    };
    if name[0] == b'.'
        || (f.attr & 0x10 != 0) != directory
        || map.chain(&disk.bytes, f.cluster).is_none()
    {
        return false;
    }
    if directory {
        if f.cluster < 2 || f.cluster == map.root {
            return false;
        }
        let Some(entries) = map.directory(&disk.bytes, f.cluster) else {
            return false;
        };
        if entries.take_while(|&at| disk.bytes[at] != 0).any(|at| {
            disk.bytes[at] != 0xe5
                && &disk.bytes[at..at + 11] != b".          "
                && &disk.bytes[at..at + 11] != b"..         "
        }) {
            return false;
        }
    }
    clear_long_name(disk, &map, &f);
    if !open::detach(dir, &f) {
        map.release(&mut disk.bytes, f.cluster);
    }
    disk.bytes.edit(f.at..f.at + 1)[0] = 0xe5;
    map.refresh_fsinfo(&mut disk.bytes);
    true
}

pub fn rename_file(dir: u32, from: &[u8; 11], to: &[u8; 11]) -> bool {
    let disk = &mut disk(dir);
    let Some(map) = layout(disk) else {
        return false;
    };
    if from[0] == b'.' || to[0] == b'.' || find(disk, &map, disk.cwd, to).is_some() {
        return false;
    }
    let Some(f) = find(disk, &map, disk.cwd, from) else {
        return false;
    };
    clear_long_name(disk, &map, &f);
    disk.bytes.edit(f.at..f.at + 11).copy_from_slice(to);
    true
}

pub fn make_dir(dir: u32, name: &[u8; 11]) -> bool {
    let disk = &mut disk(dir);
    let Some(map) = layout(disk) else {
        return false;
    };
    let parent = disk.cwd;
    if find(disk, &map, parent, name).is_some() {
        return false;
    }
    let Some(slot) = free_slot(disk, &map, parent) else {
        return false;
    };
    if !room(disk, &map, slot.extra() + 1) {
        return false;
    }
    let mut cursor = 2;
    let at = slot.claim(disk, &map, &mut cursor);
    let cluster = map.allocate(&mut disk.bytes, &mut cursor);
    let start = map.data(cluster).start;
    for (at, name, first) in [
        (at, name, cluster),
        (start, b".          ", cluster),
        (start + 32, b"..         ", parent),
    ] {
        let raw = disk.bytes.edit(at..at + 32);
        raw.fill(0);
        raw[..11].copy_from_slice(name);
        raw[11] = 0x10;
        map.set_first(raw, first);
    }
    map.refresh_fsinfo(&mut disk.bytes);
    true
}

pub fn attribute_at(dir: u32, name: &[u8; 11]) -> Option<u8> {
    let disk = &mut disk(dir);
    let map = layout(disk)?;
    Some(find(disk, &map, disk.cwd, name)?.attr)
}

pub fn set_attribute(dir: u32, name: &[u8; 11], attr: u8) -> bool {
    let disk = &mut disk(dir);
    let Some(map) = layout(disk) else {
        return false;
    };
    let Some(found) = find(disk, &map, disk.cwd, name) else {
        return false;
    };
    if found.attr & 0x10 != 0 {
        return false;
    }
    let mut raw = found.raw;
    raw[11] = attr & 0x27;
    {
        disk.bytes
            .edit(found.at..found.at + 32)
            .copy_from_slice(&raw);
        true
    }
}

pub fn exists_at(dir: u32, name: &[u8; 11]) -> bool {
    let disk = &mut disk(dir);
    let Some(map) = layout(disk) else {
        return false;
    };
    find(disk, &map, dir, name).is_some()
}

struct Found {
    dir: u32,
    index: usize,
    at: usize,
    cluster: u32,
    size: usize,
    attr: u8,
    raw: [u8; 32],
}

fn find(disk: &Disk, map: &Map, dir: u32, name: &[u8; 11]) -> Option<Found> {
    let dir = dir & !HARD;
    for (index, at) in map.directory(&disk.bytes, dir)?.enumerate() {
        let raw: [u8; 32] = disk.bytes[at..at + 32].try_into().unwrap();
        if raw[0] == 0 {
            break;
        }
        if raw[0] != 0xe5 && raw[11] & 8 == 0 && raw[..11] == *name {
            return Some(Found {
                dir,
                index,
                at,
                cluster: map.first(&raw),
                size: dword(&raw, 28) as usize,
                attr: raw[11],
                raw,
            });
        }
    }
    None
}

fn clear_long_name(disk: &mut Disk, map: &Map, f: &Found) {
    // DOS can rename/delete via the short alias of a VFAT entry.
    let checksum = f.raw[..11]
        .iter()
        .fold(0u8, |sum, &b| sum.rotate_right(1).wrapping_add(b));
    let mut slots = [0; 20];
    let start = f.index.saturating_sub(slots.len());
    let count = f.index - start;
    let Some(dir) = map.directory(&disk.bytes, f.dir) else {
        return;
    };
    for (dst, at) in slots[..count].iter_mut().zip(dir.skip(start)) {
        *dst = at;
    }
    for (i, &at) in slots[..count].iter().rev().enumerate() {
        let raw = &disk.bytes[at..at + 32];
        if raw[11] != 0x0f || raw[13] != checksum || raw[0] & 0xbf != (i + 1) as u8 {
            return;
        }
        if raw[0] & 0x40 != 0 {
            for &slot in &slots[count - 1 - i..count] {
                disk.bytes.edit(slot..slot + 1)[0] = 0xe5;
            }
            return;
        }
    }
}

enum Slot {
    At(usize),
    Grow(u32),
}
impl Slot {
    fn extra(&self) -> usize {
        usize::from(matches!(self, Self::Grow(_)))
    }
    fn claim(self, disk: &mut Disk, map: &Map, cursor: &mut u32) -> usize {
        match self {
            Self::At(at) => at,
            Self::Grow(last) => {
                let cluster = map.allocate(&mut disk.bytes, cursor);
                map.set(&mut disk.bytes, last, cluster);
                map.data(cluster).start
            }
        }
    }
}

fn free_slot(disk: &Disk, map: &Map, dir: u32) -> Option<Slot> {
    let mut slots = map.directory(&disk.bytes, dir & !HARD)?;
    let last = slots.last;
    if let Some(at) = slots.find(|&at| matches!(disk.bytes[at], 0 | 0xe5)) {
        return Some(Slot::At(at));
    }
    (last != 0).then_some(Slot::Grow(last))
}

fn room(disk: &Disk, map: &Map, need: usize) -> bool {
    map.free(&disk.bytes, 2).take(need).count() == need
}

fn parent_of(disk: &Disk, map: &Map, cluster: u32) -> u32 {
    find(disk, map, cluster, b"..         ")
        .map_or(0, |f| if f.cluster == map.root { 0 } else { f.cluster })
}
pub fn current_path(dir: u32, out: &mut [u8]) -> usize {
    let disk = &mut disk(dir);
    if disk.cwd < 2 {
        return 0;
    }
    let Some(map) = layout(disk) else {
        return 0;
    };
    let mut names = [[b' '; 11]; 16];
    let mut depth = 0usize;
    let mut cluster = disk.cwd;
    while cluster >= 2 && depth < names.len() {
        let parent = parent_of(disk, &map, cluster);
        let Some(name) = named_in(disk, &map, parent, cluster) else {
            break;
        };
        names[depth] = name;
        depth += 1;
        cluster = parent;
    }
    let mut used = 0usize;
    while depth > 0 {
        depth -= 1;
        if used > 0 {
            if used >= out.len() {
                break;
            }
            out[used] = b'\\';
            used += 1;
        }
        used = write_component(out, used, &names[depth]);
    }
    used
}

fn named_in(disk: &Disk, map: &Map, parent: u32, cluster: u32) -> Option<[u8; 11]> {
    map.directory(&disk.bytes, parent)?
        .map(|at| &disk.bytes[at..at + 32])
        .take_while(|raw| raw[0] != 0)
        .find(|raw| {
            raw[0] != 0xe5 && raw[0] != b'.' && raw[11] & 0x18 == 0x10 && map.first(raw) == cluster
        })
        .map(|raw| raw[..11].try_into().unwrap())
}
fn write_component(out: &mut [u8], mut used: usize, name: &[u8; 11]) -> usize {
    let mut index = 0usize;
    while index < 8 && name[index] != b' ' && used < out.len() {
        out[used] = name[index];
        used += 1;
        index += 1;
    }
    if name[8] != b' ' && used < out.len() {
        out[used] = b'.';
        used += 1;
        index = 8;
        while index < 11 && name[index] != b' ' && used < out.len() {
            out[used] = name[index];
            used += 1;
            index += 1;
        }
    }
    used
}

pub fn parent_directory(cluster: u32) -> u32 {
    if cluster & !HARD < 2 {
        return root(cluster);
    }
    let disk = &mut disk(cluster);
    let Some(map) = layout(disk) else {
        return root(cluster);
    };
    root(cluster) | parent_of(disk, &map, cluster & !HARD)
}

pub fn find_match(dir: u32, template: &[u8; 11], search_attr: u8, start: u16) -> Option<DirMatch> {
    let disk = disk(dir);
    let map = layout(&disk)?;
    for (index, at) in map
        .directory(&disk.bytes, dir & !HARD)?
        .take(usize::from(u16::MAX))
        .enumerate()
        .skip(usize::from(start))
    {
        let raw = &disk.bytes[at..at + 32];
        if raw[0] == 0 {
            break;
        }
        let name = raw[..11].try_into().unwrap();
        if raw[0] != 0xe5
            && raw[11] != 0x0f
            && attribute_visible(raw[11], search_attr)
            && names_match(template, &name)
        {
            return Some(DirMatch {
                name,
                attr: raw[11],
                time: word(raw, 22),
                date: word(raw, 24),
                size: dword(raw, 28),
                index: index as u16,
            });
        }
    }
    None
}
pub fn directory_cluster(parent: u32, name: &[u8; 11]) -> Option<u32> {
    let disk = &mut disk(parent);
    let map = layout(disk)?;
    let found = find(disk, &map, parent, name)?;
    if found.attr & 0x10 == 0 {
        return None;
    }
    Some(root(parent) | found.cluster)
}

// Skip complete clusters without building or copying the whole file.
fn at_offset(map: &Map, image: &[u8], mut first: u32, offset: usize) -> Option<u32> {
    for _ in 0..offset / map.cluster_bytes() {
        first = map.next(image, first)?;
    }
    Some(first)
}

fn read_chain(
    disk: &Disk,
    map: &Map,
    raw: &[u8; 32],
    offset: usize,
    out: &mut [u8],
) -> Option<usize> {
    let count = out
        .len()
        .min((dword(raw, 28) as usize).saturating_sub(offset));
    if count == 0 {
        return Some(0);
    }
    let mut cluster = at_offset(map, &disk.bytes, map.first(raw), offset)?;
    let mut shift = offset % map.cluster_bytes();
    let mut used = 0;
    while used < count {
        if !map.valid(cluster) {
            return None;
        }
        let n = (count - used).min(map.cluster_bytes() - shift);
        let at = map.data(cluster).start + shift;
        out[used..used + n].copy_from_slice(&disk.bytes[at..at + n]);
        used += n;
        cluster = map.next(&disk.bytes, cluster).unwrap_or(0);
        shift = 0;
    }
    Some(count)
}

// DOS zero-length writes set the length to the current offset.
fn write_chain(disk: &mut Disk, map: &Map, raw: &mut [u8; 32], offset: usize, data: &[u8]) -> bool {
    let previous = dword(raw, 28) as usize;
    let cluster = map.first(raw);
    let Some(end) = offset
        .checked_add(data.len())
        .filter(|&n| n <= u32::MAX as usize)
    else {
        return false;
    };
    if raw[11] & 0x11 != 0 {
        return false;
    }
    let size = if data.is_empty() {
        end
    } else {
        end.max(previous)
    };
    let unit = map.cluster_bytes();
    let Some((old, mut last)) = map.chain(&disk.bytes, cluster) else {
        return false;
    };
    let needed = size.div_ceil(unit);
    if old < previous.div_ceil(unit) || !room(disk, map, needed.saturating_sub(old)) {
        return false;
    }
    let mut first = cluster;
    if needed < old {
        if needed == 0 {
            map.release(&mut disk.bytes, first);
            first = 0;
        } else {
            let keep = at_offset(map, &disk.bytes, first, (needed - 1) * unit).unwrap();
            let tail = map.next(&disk.bytes, keep).unwrap();
            map.set(&mut disk.bytes, keep, u32::MAX);
            map.release(&mut disk.bytes, tail);
        }
    } else {
        let mut cursor = 2;
        for _ in old..needed {
            let next = map.allocate(&mut disk.bytes, &mut cursor);
            if last != 0 {
                map.set(&mut disk.bytes, last, next);
            } else {
                first = next;
            }
            last = next;
        }
    }
    let mut pos = offset.min(previous);
    if pos < end {
        let mut cluster = at_offset(map, &disk.bytes, first, pos).unwrap();
        while pos < end {
            let shift = pos % unit;
            let n = (unit - shift).min(end - pos);
            let at = map.data(cluster).start + shift;
            let gap = offset.saturating_sub(pos).min(n);
            disk.bytes.edit(at..at + gap).fill(0);
            if n > gap {
                let source = pos + gap - offset;
                disk.bytes
                    .edit(at + gap..at + n)
                    .copy_from_slice(&data[source..source + n - gap]);
            }
            pos += n;
            cluster = map.next(&disk.bytes, cluster).unwrap_or(0);
        }
    }
    raw[11] |= 0x20;
    raw[28..32].copy_from_slice(&(size as u32).to_le_bytes());
    map.set_first(raw, first);
    if old != needed {
        map.refresh_fsinfo(&mut disk.bytes);
    }
    true
}

use super::driver::Name;
use dosrs_native_posix::{Errno, Open, Result as PosixResult};
// Reject names the DOS short-name parser would otherwise silently truncate.
fn path_name(base: u32, tail: &[u8]) -> PosixResult<(u32, [u8; 11])> {
    let path = core::str::from_utf8(tail).map_err(|_| Errno::Invalid)?;
    let mut parts = path.split(['/', '\\']).peekable();
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
    super::dos::file_in(base, tail).ok_or(Errno::NoEntry)
}

fn resolve(dir: u32, path: Name) -> PosixResult<(u32, [u8; 11])> {
    match path {
        Name::Dos(n) => Ok((dir, *n)),
        Name::Native(n) => path_name(dir, n),
    }
}
fn open_vfs(dir: u32, path: Name, flags: Open) -> PosixResult<(super::File, bool)> {
    let (dir, name) = resolve(dir, path)?;
    let name = &name;
    if flags.create && flags.exclusive && exists_at(dir, name) {
        return Err(Errno::Exists);
    }
    let mut created = false;
    let object = match OpenFile::open(dir, name, flags.access.writable()) {
        Ok(object) => object,
        Err(FileError::Missing) if flags.create => {
            if !write_named_at(dir, name, &[], 0x20) {
                return Err(Errno::NoSpace);
            }
            match OpenFile::open(dir, name, flags.access.writable()) {
                Ok(object) => {
                    created = true;
                    object
                }
                Err(err) => {
                    remove_file_at(dir, name);
                    return Err(file_error(err));
                }
            }
        }
        Err(e) => return Err(file_error(e)),
    };
    if flags.truncate && !created {
        object.write(0, &[]).map_err(file_error)?;
    }
    Ok((alloc::boxed::Box::new(object), created || flags.truncate))
}
fn unlink_vfs(dir: u32, path: Name) -> PosixResult<()> {
    let (dir, name) = resolve(dir, path)?;
    let attr = attribute_at(dir, &name).ok_or(Errno::NoEntry)?;
    if attr & 0x10 != 0 {
        return Err(Errno::IsDirectory);
    }
    if attr & 1 != 0 {
        return Err(Errno::Access);
    }
    if remove_file_at(dir, &name) && flush(dir) {
        Ok(())
    } else {
        Err(Errno::Io)
    }
}

use super::driver::{Volume, VolumeId};
pub(super) struct Driver;
impl Volume for Driver {
    fn open(
        &self,
        dir: u32,
        path: Name,
        flags: Open,
    ) -> dosrs_native_posix::Result<(super::File, bool)> {
        open_vfs(dir, path, flags)
    }
    fn unlink(&self, dir: u32, path: Name) -> dosrs_native_posix::Result<()> {
        unlink_vfs(dir, path)
    }
    fn child(&self, dir: u32, name: &[u8; 11]) -> Option<u32> {
        directory_cluster(dir, name)
    }
    fn parent(&self, dir: u32) -> u32 {
        parent_directory(dir)
    }
    fn path(&self, dir: u32, out: &mut [u8]) -> usize {
        current_path(dir, out)
    }
    fn find(&self, dir: u32, pattern: &[u8; 11], attr: u8, start: u16) -> Option<DirMatch> {
        find_match(dir, pattern, attr, start)
    }
    fn attr(&self, dir: u32, name: &[u8; 11]) -> Option<u8> {
        attribute_at(dir, name)
    }
    fn set_attr(&self, dir: u32, name: &[u8; 11], attr: u8) -> bool {
        set_attribute(dir, name, attr)
    }
    fn replace(&self, dir: u32, name: &[u8; 11], data: &[u8], attr: u8) -> bool {
        write_named_at(dir, name, data, attr)
    }

    fn mkdir(&self, dir: u32, name: &[u8; 11]) -> bool {
        make_dir(dir, name)
    }
    fn rmdir(&self, dir: u32, name: &[u8; 11]) -> bool {
        remove_dir(dir, name)
    }
    fn rename(&self, dir: u32, from: &[u8; 11], to: &[u8; 11]) -> bool {
        rename_file(dir, from, to)
    }
    fn media(&self, dir: u32) -> u8 {
        media_id(dir)
    }
    fn volume(&self, dir: u32) -> Option<VolumeId> {
        volume_id(dir)
    }
    fn set_volume(&self, dir: u32, serial: &[u8; 4], label: &[u8; 11]) -> bool {
        set_volume_id(dir, serial, label)
    }
    fn parameters(&self, dir: u32) -> Option<DriveBlock> {
        drive_block(dir)
    }
    fn space(&self, dir: u32) -> Option<(u32, u32, u32, u32)> {
        space(dir)
    }
    fn flush(&self, dir: u32) -> bool {
        flush(dir)
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

impl super::Node for OpenFile {
    fn len(&self) -> PosixResult<u64> {
        Ok(self.len() as u64)
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> PosixResult<usize> {
        if offset > u64::from(u32::MAX) {
            return Ok(0);
        }
        self.read(offset as usize, out).map_err(file_error)
    }
    fn write(&self, offset: u64, data: &[u8]) -> PosixResult<()> {
        if offset.saturating_add(data.len() as u64) > u64::from(u32::MAX) {
            return Err(Errno::FileTooLarge);
        }
        self.write(offset as usize, data).map_err(file_error)
    }
    fn sync(&self) -> PosixResult<()> {
        self.sync().then_some(()).ok_or(Errno::Io)
    }
    fn identity(&self) -> (u32, u64) {
        (self.volume(), self.identity() as u64)
    }
    fn stamp(&self) -> PosixResult<(u16, u16)> {
        Ok(self.stamp())
    }
    fn set_stamp(&self, time: u16, date: u16) -> PosixResult<()> {
        self.set_stamp(time, date);
        Ok(())
    }
}
