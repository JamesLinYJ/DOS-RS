//! Data partitions on 512-byte-sector MBR and GPT disks.
use super::{BlockRead, Error};
use alloc::{vec, vec::Vec};

#[derive(Copy, Clone)]
pub struct Partition {
    pub first: u64,
    pub sectors: u64,
    pub read_only: bool,
}
impl Partition {
    fn new(first: u64, sectors: u64, limit: u64, read_only: bool) -> Result<Self, Error> {
        if first == 0 || sectors == 0 || first.checked_add(sectors).is_none_or(|end| end > limit) {
            return Err(Error);
        }
        Ok(Self {
            first,
            sectors,
            read_only,
        })
    }
}
fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap())
}
fn u64le(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[..8].try_into().unwrap())
}
fn crc32(b: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &v in b {
        crc ^= u32::from(v);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}
fn verified(entries: Vec<(Partition, bool)>) -> Result<Vec<Partition>, Error> {
    let mut by_start: Vec<_> = entries.iter().map(|(p, _)| *p).collect();
    by_start.sort_unstable_by_key(|p| p.first);
    if by_start
        .windows(2)
        .any(|p| p[0].first + p[0].sectors > p[1].first)
    {
        return Err(Error);
    }
    Ok(entries
        .into_iter()
        .filter(|(_, data)| *data)
        .map(|(p, _)| p)
        .collect())
}
pub fn data_partitions(dev: &dyn BlockRead) -> Result<Option<Vec<Partition>>, Error> {
    let limit = dev.size_bytes() / 512;
    let mut mbr = [0u8; 512];
    dev.read_at(0, &mut mbr)?;
    if mbr[510..] != [0x55, 0xaa] {
        return Ok(None);
    }
    let table = &mbr[446..510];
    if table
        .chunks_exact(16)
        .any(|e| e[4] == 0xee && u32le(&e[8..]) == 1)
    {
        return gpt(dev, limit).map(Some);
    }
    // Raw DOS and NTFS boot sectors can contain bootstrap code in the MBR slot area.
    let bps = u16::from_le_bytes([mbr[11], mbr[12]]);
    let fat_bpb = matches!(mbr[0], 0xeb | 0xe9)
        && bps == 512
        && mbr[13].is_power_of_two()
        && u16::from_le_bytes([mbr[14], mbr[15]]) != 0
        && matches!(mbr[16], 1 | 2);
    if &mbr[3..11] == b"NTFS    " || fat_bpb || table.iter().all(|b| *b == 0) {
        return Ok(None);
    }
    let mut entries = Vec::new();
    for e in table.chunks_exact(16).filter(|e| e[4] != 0) {
        if !matches!(e[0], 0 | 0x80) {
            return Err(Error);
        }
        let p = Partition::new(
            u64::from(u32le(&e[8..])),
            u64::from(u32le(&e[12..])),
            limit,
            false,
        )?;
        entries.push((
            p,
            matches!(e[4], 0x01 | 0x04 | 0x06 | 0x07 | 0x0b | 0x0c | 0x0e | 0x83),
        ));
    }
    verified(entries).map(Some)
}
fn gpt(dev: &dyn BlockRead, limit: u64) -> Result<Vec<Partition>, Error> {
    let mut h = [0u8; 512];
    dev.read_at(512, &mut h)?;
    let size = u32le(&h[12..]) as usize;
    if &h[..8] != b"EFI PART"
        || u32le(&h[8..]) != 0x10000
        || !(92..=512).contains(&size)
        || u64le(&h[24..]) != 1
    {
        return Err(Error);
    }
    let sum = u32le(&h[16..]);
    h[16..20].fill(0);
    if crc32(&h[..size]) != sum {
        return Err(Error);
    }
    let first = u64le(&h[40..]);
    let last = u64le(&h[48..]);
    let backup = u64le(&h[32..]);
    if first > last || last >= backup || backup >= limit {
        return Err(Error);
    }
    let table = u64le(&h[72..]);
    let count = u32le(&h[80..]) as usize;
    let stride = u32le(&h[84..]) as usize;
    if table < 2 || stride < 128 || !stride.is_power_of_two() || count == 0 {
        return Err(Error);
    }
    let bytes = count
        .checked_mul(stride)
        .filter(|n| *n <= 1 << 20)
        .ok_or(Error)?;
    if table
        .checked_add((bytes as u64).div_ceil(512))
        .is_none_or(|end| end > first)
    {
        return Err(Error);
    }
    let mut raw = vec![0; bytes];
    dev.read_at(table.checked_mul(512).ok_or(Error)?, &mut raw)?;
    if crc32(&raw) != u32le(&h[88..]) {
        return Err(Error);
    }
    let mut entries = Vec::new();
    const BASIC: [u8; 16] = [
        0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99,
        0xc7,
    ];
    const LINUX: [u8; 16] = [
        0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d,
        0xe4,
    ];
    for e in raw.chunks_exact(stride).filter(|e| e[..16] != [0; 16]) {
        let start = u64le(&e[32..]);
        let end = u64le(&e[40..]);
        let attrs = u64le(&e[48..]);
        if start < first || end > last || start > end {
            return Err(Error);
        }
        let p = Partition::new(start, end - start + 1, limit, attrs & (1 << 60) != 0)?;
        let data = (e[..16] == BASIC || e[..16] == LINUX) && attrs & ((1 << 62) | (1 << 63)) == 0;
        entries.push((p, data));
    }
    verified(entries)
}
