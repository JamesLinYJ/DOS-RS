//! Transfer 512-byte sectors from a primary-channel ATA master using polled PIO.
use crate::arch::{inb, inw, outb, outw};

const DATA: u16 = 0x1f0;
const STATUS: u16 = 0x1f7;

fn select(high: u8) -> bool {
    outb(0x1f6, 0xe0 | high);
    // Four alternate-status reads allow the selected device to settle.
    for _ in 0..4 {
        let _ = inb(0x3f6);
    }
    for _ in 0..65536 {
        match inb(STATUS) {
            0 | 0xff => return false,
            status if status & 0x80 == 0 => return true,
            _ => {}
        }
    }
    false
}

fn read_data(command: u8, sector: &mut [u8; 512]) -> bool {
    outb(STATUS, command);
    if !wait(true) {
        return false;
    }
    for word in sector.chunks_exact_mut(2) {
        word.copy_from_slice(&inw(DATA).to_le_bytes());
    }
    true
}

fn wait(data: bool) -> bool {
    for _ in 0..4 {
        let _ = inb(0x3f6);
    }
    for _ in 0..1_000_000 {
        let status = inb(STATUS);
        if status == 0 || status == 0xff {
            return false;
        }
        if status & 0x80 == 0 {
            if status & 0x21 != 0 {
                return false;
            }
            if (status & 8 != 0) == data {
                return true;
            }
        }
    }
    false
}

pub(crate) fn capacity() -> Option<u32> {
    if !select(0) {
        return None;
    }
    for port in 0x1f2..=0x1f5 {
        outb(port, 0);
    }
    let mut id = [0u8; 512];
    if !read_data(0xec, &mut id) || id[99] & 2 == 0 {
        return None;
    }
    let count = u32::from_le_bytes(id[120..124].try_into().ok()?).min(1 << 28);
    (count > 0).then_some(count)
}

fn address(lba: u32, sectors: u8) -> bool {
    if lba >= 1 << 28 || !select((lba >> 24) as u8) {
        return false;
    }
    let [lo, mid, hi, _] = lba.to_le_bytes();
    outb(0x1f2, sectors);
    outb(0x1f3, lo);
    outb(0x1f4, mid);
    outb(0x1f5, hi);
    true
}

pub(crate) fn read(lba: u32, sector: &mut [u8; 512]) -> bool {
    address(lba, 1) && read_data(0x20, sector)
}

pub(crate) fn read_sectors(mut lba: u32, out: &mut [u8]) -> bool {
    if !out.len().is_multiple_of(512) {
        return false;
    }
    for batch in out.chunks_mut(128 * 512) {
        let count = (batch.len() / 512) as u8;
        if !address(lba, count) {
            return false;
        }
        outb(STATUS, 0x20);
        for sector in batch.chunks_exact_mut(512) {
            if !wait(true) {
                return false;
            }
            for word in sector.chunks_exact_mut(2) {
                word.copy_from_slice(&inw(DATA).to_le_bytes());
            }
        }
        lba += u32::from(count);
    }
    true
}

pub(crate) fn write(lba: u32, sector: &[u8; 512]) -> bool {
    if !address(lba, 1) {
        return false;
    }
    outb(STATUS, 0x30);
    if !wait(true) {
        return false;
    }
    for word in sector.chunks_exact(2) {
        outw(DATA, u16::from_le_bytes([word[0], word[1]]));
    }
    wait(false)
}

pub(crate) fn flush() -> bool {
    if !select(0) {
        return false;
    }
    outb(STATUS, 0xe7);
    wait(false)
}
