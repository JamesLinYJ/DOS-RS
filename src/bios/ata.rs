//! Read 512-byte sectors from a primary-channel ATA master using polled PIO.
use crate::arch::{inb, inw, outb};

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
    for _ in 0..4 {
        let _ = inb(0x3f6);
    }
    for _ in 0..65536 {
        let status = inb(STATUS);
        if status == 0 || status == 0xff {
            return false;
        }
        if status & 0x80 != 0 {
            continue;
        }
        if status & 0x21 != 0 {
            return false;
        }
        if status & 8 != 0 {
            for word in sector.chunks_exact_mut(2) {
                word.copy_from_slice(&inw(DATA).to_le_bytes());
            }
            return true;
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

pub(crate) fn read(lba: u32, sector: &mut [u8; 512]) -> bool {
    if lba >= 1 << 28 || !select((lba >> 24) as u8) {
        return false;
    }
    let [lo, mid, hi, _] = lba.to_le_bytes();
    outb(0x1f2, 1);
    outb(0x1f3, lo);
    outb(0x1f4, mid);
    outb(0x1f5, hi);
    read_data(0x20, sector)
}
