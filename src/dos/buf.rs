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
    wrtcnt: u8,
    wrtinc: u16,
}

struct Cache {
    bufs: [Buf; NBUF],
    head: [usize; 2],
    last: Option<usize>,
}

impl Cache {
    fn new() -> Self {
        Self {
            bufs: [Buf {
                sector: 0,
                id: 0xff,
                flags: 0,
                data: [0; SECTOR],
                wrtcnt: 1,
                wrtinc: 1,
            }; NBUF],
            head: [0, BUCKET],
            last: None,
        }
    }

    fn place(&mut self, hash: usize, index: usize) {
        self.head[hash] = if self.bufs[index].id == 0xff {
            index
        } else {
            index ^ 1
        };
    }

    fn get_buffer(&mut self, dpb: &Dpb, sector: u32, fat: bool) -> Option<(usize, bool)> {
        let hash = (sector as u16 % HASH_COUNT) as usize;
        let head = self.head[hash];
        for index in [head, head ^ 1] {
            let buf = &self.bufs[index];
            if buf.id == dpb.drive && buf.sector == sector {
                let quick = self.last == Some(index);
                self.place(hash, index);
                self.last = Some(index);
                return Some((index, quick));
            }
        }
        let buf = &mut self.bufs[head];
        if buf.id != 0xff && buf.flags & DIRTY != 0 {
            for i in 0..buf.wrtcnt {
                let target = buf
                    .sector
                    .wrapping_add(u32::from(i) * u32::from(buf.wrtinc));
                if !disk::dwrite(target, 1, &buf.data) {
                    return None;
                }
            }
        }
        let ok = if fat {
            disk::fat_sec_read(dpb, sector, &mut buf.data)
        } else {
            disk::dread(sector, 1, &mut buf.data)
        };
        if !ok {
            return None;
        }
        buf.sector = sector;
        buf.id = dpb.drive;
        buf.flags = 0;
        buf.wrtcnt = if fat { dpb.fat_count.max(1) } else { 1 };
        buf.wrtinc = if fat { dpb.fat_size.max(1) } else { 1 };
        self.place(hash, head);
        self.last = Some(head);
        Some((head, false))
    }
}

/// Build the boot DPB, cache the FAT, dirty it, and read the root directory.
pub fn show() {
    let mut cache = Cache::new();
    let image = include_bytes!("../../disks/msload.img");
    let dpb = misc::set_dpb(&image[11..], 0, 0);
    let Some((fat, _)) = cache.get_buffer(&dpb, u32::from(dpb.first_fat), true) else {
        crate::bios::mscon::con_write_bytes(b"BUFFAIL\r\n");
        return;
    };
    let fat0 = cache.bufs[fat].data[0];
    let fat1 = cache.bufs[fat].data[1];
    cache.bufs[fat].data[0] = 0x11;
    cache.bufs[fat].flags |= DIRTY;
    let dir_sector = disk::dir_block(0, u32::from(dpb.dir_sector), 0);
    let Some((dir, _)) = cache.get_buffer(&dpb, dir_sector, false) else {
        crate::bios::mscon::con_write_bytes(b"BUFFAIL\r\n");
        return;
    };
    let dir0 = cache.bufs[dir].data[0];
    let dir1 = cache.bufs[dir].data[1];
    let _ = cache.get_buffer(&dpb, u32::from(dpb.first_fat).wrapping_add(4), false);
    let Some((again, _)) = cache.get_buffer(&dpb, u32::from(dpb.first_fat), true) else {
        crate::bios::mscon::con_write_bytes(b"BUFFAIL\r\n");
        return;
    };
    let ev = cache.bufs[again].data[0];
    let (_again2, hit) = cache
        .get_buffer(&dpb, u32::from(dpb.first_fat), true)
        .unwrap_or((again, false));
    cache.bufs[again].id = 0xff;
    let hash = (dpb.first_fat % HASH_COUNT) as usize;
    cache.place(hash, again);
    let head_ok = cache.head[hash] == again;

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
