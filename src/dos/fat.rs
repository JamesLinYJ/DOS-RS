//! `IsEof`, `UNPACK`, `PACK`, and `MAPCLUSTER` from `DOS/FAT.ASM`.
//!
//! The boot floppy is FAT12: 60 data clusters, so the highest cluster is 61,
//! which is below 4086. A separate table with that limit at 4086 takes the
//! FAT16 compare. `ALLOC.ASM` is the memory arena. Cluster allocate and
//! release live in `DOS/ROM.ASM`.

pub const FAT_LIMIT: u16 = 4096 - 10;

const IMAGE: &[u8] = include_bytes!("../../disks/msload.img");

/// Highest cluster number on the boot image. Data clusters are numbered from 2.
pub fn boot_max_cluster() -> u16 {
    let bps = u16::from_le_bytes([IMAGE[11], IMAGE[12]]) as usize;
    let spc = IMAGE[13] as usize;
    let reserved = u16::from_le_bytes([IMAGE[14], IMAGE[15]]) as usize;
    let fats = IMAGE[16] as usize;
    let root = u16::from_le_bytes([IMAGE[17], IMAGE[18]]) as usize;
    let total = u16::from_le_bytes([IMAGE[19], IMAGE[20]]) as usize;
    let spf = u16::from_le_bytes([IMAGE[22], IMAGE[23]]) as usize;
    let root_secs = (root * 32).div_ceil(bps);
    let data = total - reserved - fats * spf - root_secs;
    (data / spc) as u16 + 1
}

pub fn boot_fat() -> &'static [u8] {
    let reserved = u16::from_le_bytes([IMAGE[14], IMAGE[15]]) as usize;
    let bps = u16::from_le_bytes([IMAGE[11], IMAGE[12]]) as usize;
    let start = reserved * bps;
    &IMAGE[start..start + bps]
}

/// `JAE` after the 12-bit or 16-bit compare. `0x0FF0` is an end mark on FAT12.
pub fn is_eof(max_cluster: u16, value: u16) -> bool {
    if max_cluster >= FAT_LIMIT {
        value >= 0xfff8
    } else {
        value == 0x0ff0 || value >= 0x0ff8
    }
}

/// Byte offset of a cluster and whether its 12 bits sit in the high half.
pub fn map_cluster(max_cluster: u16, cluster: u16) -> Option<(usize, bool)> {
    if cluster > max_cluster {
        return None;
    }
    let fat16 = max_cluster >= FAT_LIMIT;
    let mut offset = u32::from(cluster);
    if !fat16 {
        offset >>= 1;
    }
    offset += u32::from(cluster);
    let high = !fat16 && cluster & 1 != 0;
    Some((offset as usize, high))
}

/// `UNPACK`. `None` is the bad-cluster path. Zero means a free cluster.
pub fn unpack(fat: &[u8], max_cluster: u16, cluster: u16) -> Option<u16> {
    let (offset, high) = map_cluster(max_cluster, cluster)?;
    if offset + 1 >= fat.len() {
        return None;
    }
    let mut di = u16::from_le_bytes([fat[offset], fat[offset + 1]]);
    if high {
        di >>= 4;
    } else if max_cluster < FAT_LIMIT {
        di &= 0x0fff;
    }
    Some(di)
}

/// `PACK`. The other nibble of a shared FAT12 word stays in place.
pub fn pack(fat: &mut [u8], max_cluster: u16, cluster: u16, data: u16) -> bool {
    let Some((offset, high)) = map_cluster(max_cluster, cluster) else {
        return false;
    };
    if offset + 1 >= fat.len() {
        return false;
    }
    let mut si = u16::from_le_bytes([fat[offset], fat[offset + 1]]);
    let mut dx = data;
    if high {
        dx <<= 4;
        si &= 0x000f;
    } else if max_cluster >= FAT_LIMIT {
        si = 0;
    } else {
        si &= 0xf000;
        dx &= 0x0fff;
    }
    si |= dx;
    let bytes = si.to_le_bytes();
    fat[offset] = bytes[0];
    fat[offset + 1] = bytes[1];
    true
}

/// Drive block the media check in `FAT_GOT_DPB` updates.
pub struct Dpb {
    pub first_access: u8,
    pub free_cnt: i16,
    pub next_free: u16,
}

/// Result of `FAT_GOT_DPB` after the driver media byte is known.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Media {
    /// Return byte 1. The disk is the same one.
    Unchanged,
    /// `NEWDSK`. Free space is unknown and the next-free cluster restarts.
    Changed,
    /// Return byte 0 and a dirty buffer. The disk is left alone.
    Dirty,
    /// The driver status was an error and the user failed the call.
    Fail,
}

/// `FAT_GOT_DPB`. `returned` is the media-check byte: 1, 0, or 0xFF.
/// `status_err` is `STERR` in the request status.
pub fn fat_got_dpb(dpb: &mut Dpb, returned: u8, status_err: bool, dirty: bool) -> Media {
    if status_err {
        dpb.free_cnt = -1;
        return Media::Fail;
    }
    let mixed = dpb.first_access | returned;
    dpb.first_access = 0;
    if mixed & 0x80 != 0 {
        return media_changed(dpb);
    }
    if mixed == 0 {
        if dirty {
            return Media::Dirty;
        }
        return media_changed(dpb);
    }
    Media::Unchanged
}

fn media_changed(dpb: &mut Dpb) -> Media {
    dpb.free_cnt = -1;
    dpb.next_free = 0;
    Media::Changed
}

/// `FATREAD_CDS` after a media change. A zero id is the root and stays.
pub fn fatread_cds(ids: &mut [u16], changed: bool) {
    if !changed {
        return;
    }
    let mut index = 0usize;
    while index < ids.len() {
        if ids[index] != 0 {
            ids[index] = 0xffff;
        }
        index += 1;
    }
}

/// The media-check branches that follow `MAPCLUSTER`.
pub fn media_show() {
    let nc = check(0, 1, false, false);
    let chg = check(0, 0xff, false, false);
    let mut ids = [0u16, 0x1234];
    fatread_cds(&mut ids, chg.0 == Media::Changed);
    let dty = check(0, 0, false, true);
    let err = check(0, 1, true, false);
    let fst = check(0xff, 1, false, false);
    crate::bios::mscon::con_write_bytes(b"FATMED ");
    crate::bios::mscon::con_write_bytes(label(nc));
    crate::bios::mscon::con_write_bytes(b" ");
    crate::bios::mscon::con_write_bytes(label(chg));
    crate::bios::mscon::con_write_bytes(b" CDS");
    put_hex16(ids[1]);
    crate::bios::mscon::con_write_bytes(b" ");
    crate::bios::mscon::con_write_bytes(label(dty));
    crate::bios::mscon::con_write_bytes(b" ");
    crate::bios::mscon::con_write_bytes(label(err));
    crate::bios::mscon::con_write_bytes(b" ");
    crate::bios::mscon::con_write_bytes(if fst.0 == Media::Changed && fst.1 == -1 && fst.2 == 0 {
        b"FST\r\n"
    } else {
        b"BAD\r\n"
    });
}

fn check(first: u8, returned: u8, status_err: bool, dirty: bool) -> (Media, i16, u16) {
    let mut dpb = Dpb {
        first_access: first,
        free_cnt: 4,
        next_free: 2,
    };
    let media = fat_got_dpb(&mut dpb, returned, status_err, dirty);
    (media, dpb.free_cnt, dpb.next_free)
}

fn label(result: (Media, i16, u16)) -> &'static [u8] {
    match result {
        (Media::Unchanged, 4, 2) => b"NC",
        (Media::Changed, -1, 0) => b"CHG",
        (Media::Dirty, 4, 2) => b"DTY",
        (Media::Fail, -1, 2) => b"ERR",
        _ => b"BAD",
    }
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
