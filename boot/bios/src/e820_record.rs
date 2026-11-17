//! The BIOS may return the original 20-byte record or an ACPI extended record.
use crate::E820MemoryRegion;

pub fn buffer() -> [u8; 24] {
    let mut result = [0; 24];
    result[20] = 1; // Pre-set AddressRangeEnabled for BIOSes that leave it untouched.
    result
}

pub fn decode(buf: &[u8; 24], length: usize) -> Result<Option<E820MemoryRegion>, ()> {
    let attributes = match length {
        0 => return Ok(None),
        20 => 1,
        24 => u32::from_le_bytes(buf[20..24].try_into().unwrap()),
        _ => return Err(()),
    };
    if attributes & 1 == 0 {
        return Ok(None);
    }
    let len = u64::from_le_bytes(buf[8..16].try_into().unwrap());
    Ok((len != 0).then(|| E820MemoryRegion {
        start_addr: u64::from_le_bytes(buf[..8].try_into().unwrap()),
        len,
        region_type: u32::from_le_bytes(buf[16..20].try_into().unwrap()),
        acpi_extended_attributes: attributes,
    }))
}

/// Stage only in a fully covered interval, even when firmware splits a
/// usable region or describes reserved memory inside a larger RAM entry.
pub fn can_stage(regions: &[E820MemoryRegion], mut start: u64, end: u64) -> bool {
    let last = |r: &E820MemoryRegion| r.start_addr.saturating_add(r.len);
    if regions
        .iter()
        .any(|r| !r.is_usable() && r.start_addr < end && last(r) > start)
    {
        return false;
    }
    while start < end {
        let Some(next) = regions
            .iter()
            .filter(|r| r.is_usable() && r.start_addr <= start)
            .map(last)
            .max()
            .filter(|n| *n > start)
        else {
            return false;
        };
        start = next;
    }
    true
}
