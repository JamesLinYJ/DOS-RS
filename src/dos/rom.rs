//! `ALLOCATE`, `RELEASE`, and `RELBLKS` from `DOS/ROM.ASM`.
//!
//! `ALLOCATE` walks from `dpb_next_free`, marks a free cluster with the
//! temporary value 1, and links it onto the chain. The last new cluster is
//! then stored as end-of-file. `RELBLKS` writes end-of-file into the head
//! cluster and `RELEASE` frees every cluster after it.

use super::fat::{self, FAT_LIMIT};

struct Vol {
    max: u16,
    next_free: u16,
    free_cnt: i16,
    fatbyt: u16,
}

/// BX is the last cluster already in the file. CX is how many to add.
/// The returned cluster is the first one linked on.
fn allocate(fat: &mut [u8], vol: &mut Vol, last: u16, count: u16) -> Option<u16> {
    let saved = fat::unpack(fat, vol.max, 0)?;
    vol.fatbyt = saved;
    let mut cx = count;
    let mut dx = last;
    let mut bx = vol.next_free;
    if bx <= 2 {
        vol.next_free = 2;
        bx = 1;
    }
    loop {
        bx = bx.wrapping_add(1);
        if bx > vol.max {
            if vol.next_free == 2 {
                let _ = relblks(fat, vol, last);
                let _ = restfatbyt(fat, vol);
                return None;
            }
            vol.next_free = 2;
            bx = 1;
            continue;
        }
        let link = fat::unpack(fat, vol.max, bx)?;
        if link != 0 {
            continue;
        }
        vol.next_free = bx;
        if !fat::pack(fat, vol.max, bx, 1) {
            return None;
        }
        if vol.free_cnt != -1 {
            vol.free_cnt -= 1;
        }
        if !fat::pack(fat, vol.max, dx, bx) {
            return None;
        }
        dx = bx;
        cx = cx.wrapping_sub(1);
        if cx == 0 {
            break;
        }
    }
    if !fat::pack(fat, vol.max, bx, 0xffff) {
        return None;
    }
    let first = fat::unpack(fat, vol.max, last)?;
    if !restfatbyt(fat, vol) {
        return None;
    }
    Some(first)
}

fn restfatbyt(fat: &mut [u8], vol: &Vol) -> bool {
    fat::pack(fat, vol.max, 0, vol.fatbyt)
}

/// Free the chain that starts at `cluster`. `RELBLKS` is the entry the proof uses.
#[allow(dead_code)]
fn release(fat: &mut [u8], vol: &mut Vol, cluster: u16) -> bool {
    rel_chain(fat, vol, cluster, 0)
}

/// Write end-of-file into `cluster` and free every cluster after it.
fn relblks(fat: &mut [u8], vol: &mut Vol, cluster: u16) -> bool {
    rel_chain(fat, vol, cluster, 0xffff)
}

fn rel_chain(fat: &mut [u8], vol: &mut Vol, mut cluster: u16, mut mark: u16) -> bool {
    loop {
        let Some(next) = fat::unpack(fat, vol.max, cluster) else {
            return false;
        };
        if next == 0 {
            return true;
        }
        if !fat::pack(fat, vol.max, cluster, mark) {
            return false;
        }
        if mark == 0 && vol.free_cnt != -1 {
            vol.free_cnt += 1;
        }
        cluster = next;
        mark = 0;
        if next == 1 {
            return true;
        }
        if fat::is_eof(vol.max, next) {
            return true;
        }
    }
}

/// Extend the boot image's cluster 3, release that extension, then do the
/// same on a FAT16 table whose limit is 4086.
pub fn show() {
    let mut fat12 = [0u8; 512];
    let src = fat::boot_fat();
    let n = src.len().min(fat12.len());
    fat12[..n].copy_from_slice(&src[..n]);
    let mut vol12 = Vol {
        max: fat::boot_max_cluster(),
        next_free: 2,
        free_cnt: -1,
        fatbyt: 0,
    };
    let got12 = allocate(&mut fat12, &mut vol12, 3, 1);
    let eof12 = match got12 {
        Some(cluster) => fat::unpack(&fat12, vol12.max, cluster)
            .is_some_and(|link| fat::is_eof(vol12.max, link)),
        None => false,
    };
    let _ = relblks(&mut fat12, &mut vol12, 3);
    let rel12 = fat::unpack(&fat12, vol12.max, 3).is_some_and(|link| fat::is_eof(vol12.max, link));

    let mut fat16 = [0u8; 8192];
    let mut vol16 = Vol {
        max: FAT_LIMIT,
        next_free: 2,
        free_cnt: -1,
        fatbyt: 0,
    };
    let _ = fat::pack(&mut fat16, vol16.max, 0, 0xfff8);
    let _ = fat::pack(&mut fat16, vol16.max, 1, 0xffff);
    let _ = fat::pack(&mut fat16, vol16.max, 2, 0xffff);
    let got16 = allocate(&mut fat16, &mut vol16, 2, 1);
    let eof16 = match got16 {
        Some(cluster) => fat::unpack(&fat16, vol16.max, cluster)
            .is_some_and(|link| fat::is_eof(vol16.max, link)),
        None => false,
    };
    let _ = relblks(&mut fat16, &mut vol16, 2);
    let rel16 = fat::unpack(&fat16, vol16.max, 2).is_some_and(|link| fat::is_eof(vol16.max, link));

    crate::bios::mscon::con_write_bytes(b"FAT12 ");
    put_hex16(got12.unwrap_or(0));
    crate::bios::mscon::con_write_bytes(if eof12 { b" EOF " } else { b" BAD " });
    crate::bios::mscon::con_write_bytes(if rel12 { b"REL EOF " } else { b"REL BAD " });
    crate::bios::mscon::con_write_bytes(b"FAT16 ");
    put_hex16(got16.unwrap_or(0));
    crate::bios::mscon::con_write_bytes(if eof16 { b" EOF " } else { b" BAD " });
    crate::bios::mscon::con_write_bytes(if rel16 { b"REL EOF\r\n" } else { b"REL BAD\r\n" });
}

fn put_hex16(value: u16) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[((value >> 12) & 0xf) as usize],
        hex[((value >> 8) & 0xf) as usize],
        hex[((value >> 4) & 0xf) as usize],
        hex[(value & 0xf) as usize],
    ]);
}
