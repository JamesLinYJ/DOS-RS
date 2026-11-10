//! Buffer cache from `DOS/BUF.ASM`.
//!
//! Two hash buckets, two buffers each. `GETCURHEAD` selects the bucket from
//! the low sector number. `GETBUFFR` reuses the head buffer on a miss, and
//! `PLACEBUF` makes the buffer just used the tail. `BUFWRITE` copies a dirty
//! buffer back to the disk image before that reuse.

use super::disk;
use super::misc::{self, Dpb};

const HASH_COUNT: u16 = 2;
const BUCKET: usize = 2;
const NBUF: usize = 4;
const SECTOR: usize = 512;
const DIRTY: u8 = 0x40;

#[derive(Clone, Copy)]
struct Buf {
    sector: u32,
    id: u8,
    flags: u8,
    data: [u8; SECTOR],
    next: u8,
    prev: u8,
    wrtcnt: u8,
    wrtinc: u16,
}

struct Hash {
    head: u8,
    dirty: u16,
}

static mut BUFS: [Buf; NBUF] = [Buf {
    sector: 0,
    id: 0xff,
    flags: 0,
    data: [0; SECTOR],
    next: 0,
    prev: 0,
    wrtcnt: 1,
    wrtinc: 1,
}; NBUF];
static mut HASH: [Hash; 2] = [
    Hash { head: 0, dirty: 0 },
    Hash { head: 2, dirty: 0 },
];
static mut READY: bool = false;
static mut LAST: u8 = 0xff;
static mut CUR_HASH: usize = 0;

fn ensure() {
    unsafe {
        if READY {
            return;
        }
        let mut bucket = 0usize;
        while bucket < 2 {
            let a = bucket * BUCKET;
            let b = a + 1;
            BUFS[a].next = b as u8;
            BUFS[a].prev = b as u8;
            BUFS[a].id = 0xff;
            BUFS[b].next = a as u8;
            BUFS[b].prev = a as u8;
            BUFS[b].id = 0xff;
            HASH[bucket].head = a as u8;
            HASH[bucket].dirty = 0;
            bucket += 1;
        }
        READY = true;
    }
    disk::ensure();
}

fn bucket(sector: u32) -> usize {
    (sector as u16 % HASH_COUNT) as usize
}

/// `GETCURHEAD`. The remainder of the sector number selects the hash entry.
fn cur_head(sector: u32) -> (usize, usize) {
    ensure();
    let hash = bucket(sector);
    unsafe {
        CUR_HASH = hash;
        LAST = 0xff;
        (hash, HASH[hash].head as usize)
    }
}

fn place_buf(hash: usize, destination: usize) {
    unsafe {
        let head = HASH[hash].head as usize;
        let next = BUFS[destination].next as usize;
        if next != head {
            if destination == head {
                HASH[hash].head = next as u8;
            } else {
                let prev = BUFS[destination].prev as usize;
                let nxt = next;
                BUFS[prev].next = nxt as u8;
                BUFS[nxt].prev = prev as u8;
                let last = BUFS[head].prev as usize;
                BUFS[last].next = destination as u8;
                BUFS[head].prev = destination as u8;
                BUFS[destination].prev = last as u8;
                BUFS[destination].next = head as u8;
            }
        }
        if BUFS[destination].id == 0xff {
            HASH[hash].head = destination as u8;
        }
    }
}

/// `ScanPlace`. The returned index is the buffer that followed `di`.
pub fn scan_place(hash: usize, destination: usize) -> usize {
    ensure();
    let saved = unsafe { BUFS[destination].next as usize };
    place_buf(hash, destination);
    saved
}

/// `BUFWRITE`. A free or clean buffer is left untouched on disk.
fn buf_write(destination: usize) -> bool {
    unsafe {
        let id = BUFS[destination].id;
        let flags = BUFS[destination].flags;
        BUFS[destination].id = 0xff;
        BUFS[destination].flags = 0;
        if id == 0xff || flags & DIRTY == 0 {
            return true;
        }
        let hash = bucket(BUFS[destination].sector);
        if HASH[hash].dirty > 0 {
            HASH[hash].dirty -= 1;
        }
        let mut buffer = BUFS[destination].sector;
        let mut left = BUFS[destination].wrtcnt.max(1);
        let step = u32::from(BUFS[destination].wrtinc.max(1));
        let mut wrote = false;
        while left > 0 {
            if disk::dwrite(buffer, 1, &BUFS[destination].data) {
                wrote = true;
            }
            buffer = buffer.wrapping_add(step);
            left -= 1;
        }
        wrote
    }
}

fn mark_dirty(destination: usize) {
    unsafe {
        if BUFS[destination].flags & DIRTY == 0 {
            BUFS[destination].flags |= DIRTY;
            let hash = bucket(BUFS[destination].sector);
            HASH[hash].dirty = HASH[hash].dirty.wrapping_add(1);
        }
    }
}

/// `GETBUFFR`. `fat` selects `FATSecRd` instead of `DREAD`.
/// The bool is the `JUSTBUF` hit on `LASTBUFFER`.
pub fn get_buffer(dpb: &Dpb, sector: u32, fat: bool) -> Option<(usize, bool)> {
    ensure();
    let drive = dpb.drive;
    unsafe {
        if LAST != 0xff {
            let last = LAST as usize;
            if BUFS[last].sector == sector && BUFS[last].id == drive {
                CUR_HASH = bucket(sector);
                return Some((last, true));
            }
        }
    }
    let (hash, first) = cur_head(sector);
    let mut destination = first;
    let mut guard = 0usize;
    loop {
        let same = unsafe { BUFS[destination].sector == sector && BUFS[destination].id == drive };
        if same {
            place_buf(hash, destination);
            unsafe { LAST = destination as u8 };
            return Some((destination, false));
        }
        destination = unsafe { BUFS[destination].next as usize };
        guard += 1;
        if destination == first || guard > NBUF {
            break;
        }
    }
    destination = first;
    if !buf_write(destination) {
        return None;
    }
    let ok = unsafe {
        if fat {
            disk::fat_sec_read(dpb, sector, &mut BUFS[destination].data)
        } else {
            disk::dread(sector, 1, &mut BUFS[destination].data)
        }
    };
    if !ok {
        return None;
    }
    unsafe {
        BUFS[destination].sector = sector;
        BUFS[destination].id = drive;
        BUFS[destination].flags = if fat { 0x02 } else { 0 };
        BUFS[destination].wrtcnt = if fat { dpb.fat_count.max(1) } else { 1 };
        BUFS[destination].wrtinc = if fat { dpb.fat_size.max(1) } else { 1 };
        LAST = destination as u8;
    }
    place_buf(hash, destination);
    Some((destination, false))
}

fn byte_at(index: usize, at: usize) -> u8 {
    unsafe { BUFS[index].data[at] }
}

/// Build the boot DPB, cache the FAT, dirty it, and read the root directory.
pub fn show() {
    let image = include_bytes!("../../disks/msload.img");
    let dpb = misc::set_dpb(&image[11..], 0, 0);
    let Some((fat, _)) = get_buffer(&dpb, u32::from(dpb.first_fat), true) else {
        crate::bios::mscon::con_write_bytes(b"BUFFAIL\r\n");
        return;
    };
    let fat0 = byte_at(fat, 0);
    let fat1 = byte_at(fat, 1);
    unsafe { BUFS[fat].data[0] = 0x11 };
    mark_dirty(fat);
    let dir_sector = disk::dir_block(0, u32::from(dpb.dir_sector), 0);
    let Some((dir, _)) = get_buffer(&dpb, dir_sector, false) else {
        crate::bios::mscon::con_write_bytes(b"BUFFAIL\r\n");
        return;
    };
    let dir0 = byte_at(dir, 0);
    let dir1 = byte_at(dir, 1);
    let _ = get_buffer(&dpb, u32::from(dpb.first_fat).wrapping_add(4), false);
    let Some((again, _)) = get_buffer(&dpb, u32::from(dpb.first_fat), true) else {
        crate::bios::mscon::con_write_bytes(b"BUFFAIL\r\n");
        return;
    };
    let ev = byte_at(again, 0);
    let (_again2, hit) = get_buffer(&dpb, u32::from(dpb.first_fat), true).unwrap_or((again, false));
    unsafe { BUFS[again].id = 0xff };
    let hash = bucket(u32::from(dpb.first_fat));
    let _saved = scan_place(hash, again);
    let head_ok = unsafe { HASH[hash].head == again as u8 };

    crate::bios::mscon::con_write_bytes(b"DPB ");
    put_hex16(dpb.sector_size);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(dpb.max_cluster);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(dpb.media);
    crate::bios::mscon::con_write_bytes(b" BUF ");
    put_hex8(fat0);
    put_hex8(fat1);
    crate::bios::mscon::con_write_bytes(b" EV");
    put_hex8(ev);
    crate::bios::mscon::con_write_bytes(if hit { b" HIT DIR " } else { b" MISS DIR " });
    crate::bios::mscon::con_write_bytes(&[dir0, dir1]);
    crate::bios::mscon::con_write_bytes(if head_ok { b" SC\r\n" } else { b" NOSC\r\n" });
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
