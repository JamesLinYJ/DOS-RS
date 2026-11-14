//! Guest boot volume shared by DOS file services and BIOS sector reads.
#[path = "disk/volume.rs"]
mod volume;
use volume::{Kind, Map, dword, word};

#[cfg(test)]
const IMAGE: &[u8] = include_bytes!("../../disks/guest12.img");
#[cfg(not(test))]
const IMAGE: &[u8] = include_bytes!(env!("DOS_RS_GUEST_IMAGE"));

#[path = "disk/media.rs"]
mod media;
use media::{Disk, HARD, disk};
pub fn in_dir<T>(dir: u32, act: impl FnOnce() -> T) -> T {
    let saved = working_directory();
    set_working(dir);
    let result = act();
    set_working(saved);
    result
}
pub fn has_c() -> bool {
    media::available()
}
pub fn flush(dir: u32) -> bool {
    media::flush(dir)
}
pub fn root(dir: u32) -> u32 {
    dir & HARD
}
pub fn drive_root(drive: u8) -> Option<u32> {
    match drive {
        0 | 1 => Some(0),
        2 if has_c() => Some(HARD),
        _ => None,
    }
}

/// DOS keeps a different current directory for each drive in each virtual machine.
#[derive(Clone, Copy)]
pub struct Directories {
    active: u8,
    paths: [u32; 3],
}
impl Directories {
    pub const ROOTS: Self = Self {
        active: 0,
        paths: [0, 0, HARD],
    };
}
static DIRECTORIES: crate::kernel_cell::KernelCell<Directories> =
    crate::kernel_cell::KernelCell::new(Directories::ROOTS);

pub fn directories() -> Directories {
    *DIRECTORIES.borrow()
}
pub fn restore(dirs: Directories) {
    set_working(dirs.paths[usize::from(dirs.active)]);
    *DIRECTORIES.borrow() = dirs;
}
pub fn drive_directory(drive: u8) -> Option<u32> {
    drive_root(drive)?;
    Some(DIRECTORIES.borrow().paths[usize::from(drive)])
}
pub fn select_drive(drive: u8) -> bool {
    let Some(cwd) = drive_directory(drive) else {
        return false;
    };
    DIRECTORIES.borrow().active = drive;
    set_working(cwd);
    true
}

fn layout(disk: &Disk) -> Option<Map> {
    Map::parse(&disk.bytes)
}
pub fn media_id() -> u8 {
    disk(working_directory()).bytes[21]
}
pub struct DriveBlock {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved: u16,
    pub fats: u8,
    pub root_entries: u16,
    pub sectors_per_fat: u16,
    pub media: u8,
    pub data_sector: u16,
    pub max_cluster: u16,
    pub shift: u8,
}

pub fn drive_block() -> Option<DriveBlock> {
    let disk = disk(working_directory());
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

pub fn volume_id() -> Option<([u8; 4], [u8; 11], [u8; 8])> {
    let disk = disk(working_directory());
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

pub fn set_volume_id(serial: &[u8; 4], label: &[u8; 11]) -> bool {
    let disk = &mut disk(working_directory());
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

pub fn space() -> Option<(u32, u32, u32, u32)> {
    let disk = disk(working_directory());
    let map = layout(&disk)?;
    Some((
        map.spc as u32,
        map.bps as u32,
        map.free(&disk.bytes, 2).count() as u32,
        map.max - 1,
    ))
}

pub fn allocation() -> Option<(u16, u16, u16, u16)> {
    allocation_at(working_directory())
}
pub fn allocation_at(dir: u32) -> Option<(u16, u16, u16, u16)> {
    let (mut spc, bps, mut free, mut total) = in_dir(dir, space)?;
    // Group allocation units for the classic 16-bit free-space interface.
    while total > 0xfffe && spc < 128 {
        total /= 2;
        free /= 2;
        spc *= 2;
    }
    Some((
        spc as u16,
        bps as u16,
        free.min(0xfffe) as u16,
        total.min(0xfffe) as u16,
    ))
}

pub(super) fn with_media<T>(read: impl FnOnce(&[u8]) -> T) -> T {
    read(&disk(0).bytes)
}
pub fn boot() {
    media::boot();
    restore(Directories::ROOTS);
}
pub fn file_len(name: &[u8; 11]) -> Option<usize> {
    file_len_at(working_directory(), name)
}

pub fn file_len_at(dir: u32, name: &[u8; 11]) -> Option<usize> {
    let disk = &disk(dir);
    let map = layout(disk)?;
    let found = find(disk, &map, dir, name)?;
    if found.attr & 0x10 != 0 {
        return None;
    }
    Some(found.size)
}

pub fn read_head(name: &[u8; 11], out: &mut [u8]) -> Option<usize> {
    let disk = &mut disk(working_directory());
    let map = layout(disk)?;
    let found = find(disk, &map, disk.cwd, name)?;
    if found.attr & 0x10 != 0 {
        return None;
    }
    let want = found.size.min(out.len());
    copy_cluster(disk, &map, found.cluster, want, out)
}

pub fn read_file(name: &[u8; 11], out: &mut [u8]) -> Option<usize> {
    read_file_at(working_directory(), name, out)
}

pub fn read_file_at(dir: u32, name: &[u8; 11], out: &mut [u8]) -> Option<usize> {
    let disk = &mut disk(dir);
    let map = layout(disk)?;
    let found = find(disk, &map, dir, name)?;
    if found.attr & 0x10 != 0 {
        return None;
    }
    copy_cluster(disk, &map, found.cluster, found.size, out)
}

pub fn write_file(name: &[u8; 11], data: &[u8]) -> bool {
    write_named(name, data, 0x20)
}

pub fn write_named(name: &[u8; 11], data: &[u8], attr: u8) -> bool {
    write_named_at(working_directory(), name, data, attr)
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

pub fn finish_fcb(name: &[u8; 11], size: u32, time: u16, date: u16) -> bool {
    let disk = &mut disk(working_directory());
    let Some(map) = layout(disk) else {
        return false;
    };
    let Some(f) = find(disk, &map, disk.cwd, name) else {
        return false;
    };
    if f.attr & 0x10 != 0 || size as usize > f.size {
        return false;
    }
    let Some((old, _)) = map.chain(&disk.bytes, f.cluster) else {
        return false;
    };
    let keep = (size as usize).div_ceil(map.cluster_bytes());
    if keep > old {
        return false;
    }
    let mut raw = f.raw;
    if keep < old {
        if keep == 0 {
            map.release(&mut disk.bytes, f.cluster);
            map.set_first(&mut raw, 0);
        } else {
            let mut last = f.cluster;
            for _ in 1..keep {
                last = map.next(&disk.bytes, last).unwrap();
            }
            let tail = map.next(&disk.bytes, last).unwrap();
            map.release(&mut disk.bytes, tail);
            map.set(&mut disk.bytes, last, u32::MAX);
        }
        map.refresh_fsinfo(&mut disk.bytes);
    }
    raw[22..24].copy_from_slice(&time.to_le_bytes());
    raw[24..26].copy_from_slice(&date.to_le_bytes());
    raw[28..32].copy_from_slice(&size.to_le_bytes());
    disk.bytes.edit(f.at..f.at + 32).copy_from_slice(&raw);
    true
}

pub fn remove_file(name: &[u8; 11]) -> bool {
    remove(name, false)
}
pub fn remove_dir(name: &[u8; 11]) -> bool {
    remove(name, true)
}
fn remove(name: &[u8; 11], directory: bool) -> bool {
    let disk = &mut disk(working_directory());
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
    map.release(&mut disk.bytes, f.cluster);
    disk.bytes.edit(f.at..f.at + 1)[0] = 0xe5;
    map.refresh_fsinfo(&mut disk.bytes);
    true
}

pub fn rename_file(from: &[u8; 11], to: &[u8; 11]) -> bool {
    let disk = &mut disk(working_directory());
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

pub fn make_dir(name: &[u8; 11]) -> bool {
    let disk = &mut disk(working_directory());
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
pub fn change_dir(path: &[u8]) -> bool {
    let Some(dir) = resolve_dir(path) else {
        return false;
    };
    let mut dirs = DIRECTORIES.borrow();
    let drive = if path.get(1) == Some(&b':') {
        path[0].to_ascii_uppercase() - b'A'
    } else {
        dirs.active
    };
    dirs.paths[usize::from(drive)] = dir;
    if drive == dirs.active {
        set_working(dir);
    }
    true
}
pub fn resolve_dir(path: &[u8]) -> Option<u32> {
    let (mut dir, path) = path_start(path)?;
    for part in path.split(|b| matches!(*b, b'/' | b'\\')) {
        if part.is_empty() || part == b"." {
            continue;
        }
        dir = if part == b".." {
            parent_directory(dir)
        } else {
            directory_cluster(dir, &canonical(part)?)?
        };
    }
    Some(dir)
}
pub fn path_start(path: &[u8]) -> Option<(u32, &[u8])> {
    let mut dir = working_directory();
    let mut tail = path;
    if path.get(1) == Some(&b':') {
        dir = drive_directory(path[0].to_ascii_uppercase().checked_sub(b'A')?)?;
        tail = &path[2..];
    }
    if tail.first().is_some_and(|b| matches!(*b, b'/' | b'\\')) {
        dir = root(dir);
    }
    Some((dir, tail))
}
pub fn path_name(path: &[u8]) -> Option<(u32, [u8; 11])> {
    let (base, tail) = path_start(path)?;
    if let Some(i) = tail.iter().rposition(|b| matches!(*b, b'/' | b'\\')) {
        let dir = in_dir(base, || resolve_dir(&tail[..i + 1]))?;
        Some((dir, canonical(&tail[i + 1..])?))
    } else {
        Some((base, canonical(tail)?))
    }
}

pub fn stamp(name: &[u8; 11]) -> Option<(u16, u16)> {
    stamp_at(working_directory(), name)
}

pub fn stamp_at(dir: u32, name: &[u8; 11]) -> Option<(u16, u16)> {
    let disk = &mut disk(dir);
    let map = layout(disk)?;
    let found = find(disk, &map, dir, name)?;
    let time = u16::from_le_bytes([found.raw[22], found.raw[23]]);
    let date = u16::from_le_bytes([found.raw[24], found.raw[25]]);
    Some((time, date))
}

pub fn set_stamp_at(dir: u32, name: &[u8; 11], time: u16, date: u16) -> bool {
    let disk = &mut disk(dir);
    let Some(map) = layout(disk) else {
        return false;
    };
    let Some(found) = find(disk, &map, dir, name) else {
        return false;
    };
    let mut raw = found.raw;
    let time_bytes = time.to_le_bytes();
    let date_bytes = date.to_le_bytes();
    raw[22] = time_bytes[0];
    raw[23] = time_bytes[1];
    raw[24] = date_bytes[0];
    raw[25] = date_bytes[1];
    {
        disk.bytes
            .edit(found.at..found.at + 32)
            .copy_from_slice(&raw);
        true
    }
}

pub fn attribute(name: &[u8; 11]) -> Option<u8> {
    let disk = &mut disk(working_directory());
    let map = layout(disk)?;
    Some(find(disk, &map, disk.cwd, name)?.attr)
}

pub fn set_attribute(name: &[u8; 11], attr: u8) -> bool {
    let disk = &mut disk(working_directory());
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

fn copy_cluster(
    disk: &Disk,
    map: &Map,
    cluster: u32,
    size: usize,
    out: &mut [u8],
) -> Option<usize> {
    let (len, _) = map.chain(&disk.bytes, cluster)?;
    if size > out.len() || size.div_ceil(map.cluster_bytes()) > len {
        return None;
    }
    let mut current = cluster;
    for chunk in out[..size].chunks_mut(map.cluster_bytes()) {
        let start = map.data(current).start;
        chunk.copy_from_slice(&disk.bytes[start..start + chunk.len()]);
        current = map.next(&disk.bytes, current).unwrap_or(0);
    }
    Some(size)
}

fn parent_of(disk: &Disk, map: &Map, cluster: u32) -> u32 {
    find(disk, map, cluster, b"..         ")
        .map_or(0, |f| if f.cluster == map.root { 0 } else { f.cluster })
}
pub fn current_path(out: &mut [u8]) -> usize {
    let disk = &mut disk(working_directory());
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

pub fn working_directory() -> u32 {
    media::cwd()
}

pub fn set_working(cluster: u32) {
    media::select(cluster);
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

pub struct DirMatch {
    pub name: [u8; 11],
    pub attr: u8,
    pub time: u16,
    pub date: u16,
    pub size: u32,
    pub index: u16,
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

fn attribute_visible(file_attr: u8, search_attr: u8) -> bool {
    const SPECIAL: u8 = 0x02 | 0x04 | 0x08 | 0x10;
    file_attr & SPECIAL & !search_attr == 0
}

fn names_match(template: &[u8; 11], name: &[u8; 11]) -> bool {
    let mut index = 0usize;
    while index < 11 {
        if template[index] != b'?' && template[index] != name[index] {
            return false;
        }
        index += 1;
    }
    true
}

pub fn canonical(text: &[u8]) -> Option<[u8; 11]> {
    let mut name = [b' '; 11];
    let mut extension = false;
    let mut base = 0usize;
    let mut ext = 0usize;
    let mut seen = false;
    for byte in text {
        if *byte == 0 || *byte == b' ' {
            break;
        }
        seen = true;
        if *byte == b'.' && !extension {
            extension = true;
            continue;
        }
        let upper = if byte.is_ascii_lowercase() {
            byte.wrapping_sub(b'a').wrapping_add(b'A')
        } else {
            *byte
        };
        if !extension && base < 8 {
            name[base] = upper;
            base += 1;
        } else if extension && ext < 3 {
            name[8 + ext] = upper;
            ext += 1;
        }
    }
    if seen { Some(name) } else { None }
}

// Skip complete clusters without building or copying the whole file.
fn at_offset(map: &Map, image: &[u8], mut first: u32, offset: usize) -> Option<u32> {
    for _ in 0..offset / map.cluster_bytes() {
        first = map.next(image, first)?;
    }
    Some(first)
}

pub fn read_range(dir: u32, name: &[u8; 11], offset: usize, out: &mut [u8]) -> Option<usize> {
    let disk = disk(dir);
    let map = layout(&disk)?;
    let f = find(&disk, &map, dir, name)?;
    if f.attr & 0x10 != 0 {
        return None;
    }
    let count = out.len().min(f.size.saturating_sub(offset));
    if count == 0 {
        return Some(0);
    }
    let mut cluster = at_offset(&map, &disk.bytes, f.cluster, offset)?;
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
pub fn write_range(dir: u32, name: &[u8; 11], offset: usize, data: &[u8]) -> bool {
    let mut disk = disk(dir);
    let Some(map) = layout(&disk) else {
        return false;
    };
    let Some(f) = find(&disk, &map, dir, name) else {
        return false;
    };
    let Some(end) = offset
        .checked_add(data.len())
        .filter(|&n| n <= u32::MAX as usize)
    else {
        return false;
    };
    if f.attr & 0x11 != 0 {
        return false;
    }
    let size = if data.is_empty() {
        end
    } else {
        end.max(f.size)
    };
    let unit = map.cluster_bytes();
    let Some((old, mut last)) = map.chain(&disk.bytes, f.cluster) else {
        return false;
    };
    let needed = size.div_ceil(unit);
    if old < f.size.div_ceil(unit) || !room(&disk, &map, needed.saturating_sub(old)) {
        return false;
    }
    let mut first = f.cluster;
    if needed < old {
        if needed == 0 {
            map.release(&mut disk.bytes, first);
            first = 0;
        } else {
            let keep = at_offset(&map, &disk.bytes, first, (needed - 1) * unit).unwrap();
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
    let mut pos = offset.min(f.size);
    if pos < end {
        let mut cluster = at_offset(&map, &disk.bytes, first, pos).unwrap();
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
    let mut raw = f.raw;
    raw[11] |= 0x20;
    raw[28..32].copy_from_slice(&(size as u32).to_le_bytes());
    map.set_first(&mut raw, first);
    disk.bytes.edit(f.at..f.at + 32).copy_from_slice(&raw);
    if old != needed {
        map.refresh_fsinfo(&mut disk.bytes);
    }
    true
}
