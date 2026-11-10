//! `$SETDPB` from `DOS/MISC.ASM`.
//!
//! The drive-parameter block itself is `INC/DPB.INC`. That file's header
//! names it `dpb.asm`. There is no separate `DPB.ASM` in this tree.
//! `FAT_operation` calls `$SETDPB` after a media change to rebuild the
//! block from the BIOS parameter block.

/// One drive parameter block. Field order follows `DPB.INC`.
#[derive(Clone, Copy)]
pub struct Dpb {
    pub drive: u8,
    pub unit: u8,
    pub sector_size: u16,
    pub cluster_mask: u8,
    pub cluster_shift: u8,
    pub first_fat: u16,
    pub fat_count: u8,
    pub root_entries: u16,
    pub first_sector: u16,
    pub max_cluster: u16,
    pub fat_size: u16,
    pub dir_sector: u16,
    pub media: u8,
    pub first_access: u8,
    pub next_free: u16,
    pub free_cnt: i16,
}

/// Build a DPB from a BPB. `ES:BP` drive and unit are left as given.
pub fn set_dpb(bpb: &[u8], drive: u8, unit: u8) -> Dpb {
    let mut si = 0usize;
    let sector_size = word(bpb, &mut si);
    let mut dpb = Dpb {
        drive,
        unit,
        sector_size,
        cluster_mask: 0,
        cluster_shift: 0,
        first_fat: 0,
        fat_count: 0,
        root_entries: 0,
        first_sector: 0,
        max_cluster: 0,
        fat_size: 0,
        dir_sector: 0,
        media: 0,
        first_access: 0xff,
        next_free: 0,
        free_cnt: -1,
    };
    // SI is at sectors per cluster. FAT count is three bytes further.
    if bpb[si + 3] == 0 {
        dpb.fat_count = 0;
        dpb.max_cluster = sector_size.wrapping_add(1);
        return dpb;
    }
    let mut dx = sector_size;
    let mut al = bpb[si];
    si += 1;
    al = al.wrapping_sub(1);
    dpb.cluster_mask = al;
    al = al.wrapping_add(1);
    let mut shift = 0u8;
    let mut guard = 0u8;
    while al & 1 == 0 && guard < 8 {
        shift = shift.wrapping_add(1);
        al >>= 1;
        guard = guard.wrapping_add(1);
    }
    dpb.cluster_shift = shift;
    dpb.first_fat = word(bpb, &mut si);
    dpb.fat_count = bpb[si];
    si += 1;
    let fats = dpb.fat_count;
    let mut ax = word(bpb, &mut si);
    dpb.root_entries = ax;
    dx >>= 5;
    ax = ax.wrapping_sub(1).wrapping_add(dx);
    let per_sector = dx;
    let dir_sectors = if per_sector == 0 { 0 } else { ax / per_sector };
    let disk_size = word(bpb, &mut si);
    dpb.media = bpb[si];
    si += 1;
    dpb.fat_size = word(bpb, &mut si);
    let mut ax = dpb.fat_size.wrapping_mul(u16::from(fats));
    ax = ax.wrapping_add(dpb.first_fat);
    dpb.dir_sector = ax;
    ax = ax.wrapping_add(dir_sectors);
    dpb.first_sector = ax;
    let clusters = if disk_size != 0 {
        let ax = dpb.first_sector.wrapping_sub(disk_size).wrapping_neg();
        ax >> shift
    } else {
        let big = u32::from(word(bpb, &mut si)) | (u32::from(word(bpb, &mut si)) << 16);
        let mut data = big.wrapping_sub(u32::from(dpb.first_sector));
        let mut n = shift;
        while n > 0 {
            data >>= 1;
            n -= 1;
        }
        data as u16
    };
    dpb.max_cluster = clusters.wrapping_add(1);
    dpb.next_free = 0;
    dpb.free_cnt = -1;
    dpb
}

fn word(bpb: &[u8], si: &mut usize) -> u16 {
    let at = *si;
    *si += 2;
    u16::from_le_bytes([bpb[at], bpb[at + 1]])
}
