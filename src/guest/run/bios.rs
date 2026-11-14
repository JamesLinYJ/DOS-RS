//! BIOS interrupt adapter for emulated CPUs.
use super::*;

pub(super) fn serve_bios(cpu: &mut CpuState, ram: &mut GuestRam, vector: u8) -> bool {
    match vector {
        0x10 => video(cpu, ram),
        0x11 => cpu.set_wide(Slot::Accumulator, 0x0421),
        0x12 => cpu.set_wide(Slot::Accumulator, 640),
        0x15 => match cpu.high(Slot::Accumulator) {
            0x24 => {
                match cpu.low(Slot::Accumulator) {
                    0 => ram.a20 = false,
                    1 => ram.a20 = true,
                    2 => cpu.set_low(Slot::Accumulator, u8::from(ram.a20)),
                    3 => cpu.set_wide(Slot::Base, 3),
                    _ => {
                        cpu.set_high(Slot::Accumulator, 0x86);
                        cpu.set_carry(true);
                        return true;
                    }
                }
                cpu.set_high(Slot::Accumulator, 0);
                cpu.set_carry(false);
            }
            0xc0 => {
                cpu.set_selector(Segment::Extra, 0xf000);
                cpu.set_wide(Slot::Base, 0xe6f5);
                cpu.set_high(Slot::Accumulator, 0);
                cpu.set_carry(false);
            }
            0x88 => {
                cpu.set_wide(
                    Slot::Accumulator,
                    ((super::super::memory::RAM_BYTES - 0x100000) / 1024) as u16,
                );
                cpu.set_carry(false);
            }
            _ => {
                cpu.set_high(Slot::Accumulator, 0x86);
                cpu.set_carry(true);
            }
        },
        0x13 => read_sectors(cpu, ram),
        0x14 => serial_bios(cpu),
        0x17 => printer_bios(cpu),
        0x16 => keyboard(cpu),
        0x1A => clock(cpu),
        _ => return false,
    }
    true
}

fn video(cpu: &mut CpuState, ram: &mut GuestRam) {
    let row = ram.load(0x40, 0x51).unwrap_or(0);
    let col = ram.load(0x40, 0x50).unwrap_or(0);
    let set_cursor = |ram: &mut GuestRam, row: u8, col: u8| {
        ram.store(0x40, 0x50, col.min(79));
        ram.store(0x40, 0x51, row.min(24));
        crate::bios::pc::position(row, col);
    };
    match cpu.high(Slot::Accumulator) {
        0 if matches!(cpu.low(Slot::Accumulator) & 0x7f, 2 | 3 | 7) => {
            let mode = cpu.low(Slot::Accumulator);
            ram.store(0x40, 0x49, mode & 0x7f);
            if mode & 0x80 == 0 {
                for r in 0..25 {
                    for c in 0..80 {
                        ram.write_text(r, c, 0x0720);
                    }
                }
            }
            set_cursor(ram, 0, 0);
            cpu.set_carry(false);
        }
        1 => {
            ram.store(0x40, 0x60, cpu.low(Slot::Count));
            ram.store(0x40, 0x61, cpu.high(Slot::Count));
            cpu.set_carry(false);
        }
        2 => {
            set_cursor(ram, cpu.high(Slot::Data), cpu.low(Slot::Data));
            cpu.set_carry(false);
        }
        3 => {
            cpu.set_wide(Slot::Data, u16::from_be_bytes([row, col]));
            cpu.set_wide(
                Slot::Count,
                u16::from_be_bytes([
                    ram.load(0x40, 0x61).unwrap_or(6),
                    ram.load(0x40, 0x60).unwrap_or(7),
                ]),
            );
            cpu.set_carry(false);
        }
        6 | 7 => {
            let (top, left) = (cpu.high(Slot::Count).min(24), cpu.low(Slot::Count).min(79));
            let (bottom, right) = (cpu.high(Slot::Data).min(24), cpu.low(Slot::Data).min(79));
            let lines = cpu.low(Slot::Accumulator);
            let down = cpu.high(Slot::Accumulator) == 7;
            let blank = u16::from_be_bytes([cpu.high(Slot::Base), 32]);
            for n in top..=bottom {
                let r = if down { bottom - (n - top) } else { n };
                for c in left..=right {
                    let source = if down {
                        r.checked_sub(lines)
                    } else {
                        r.checked_add(lines)
                    };
                    let word = if let Some(y) =
                        source.filter(|y| lines != 0 && (top..=bottom).contains(y))
                    {
                        ram.text(y, c)
                    } else {
                        blank
                    };
                    ram.write_text(r, c, word);
                }
            }
            cpu.set_carry(false);
        }
        8 => {
            cpu.set_wide(Slot::Accumulator, ram.text(row, col));
            cpu.set_carry(false);
        }
        9 | 10 => {
            let pos = usize::from(row) * 80 + usize::from(col);
            for i in pos..(pos + usize::from(cpu.wide(Slot::Count))).min(2000) {
                let (r, c) = ((i / 80) as u8, (i % 80) as u8);
                let attr = if cpu.high(Slot::Accumulator) == 9 {
                    cpu.low(Slot::Base)
                } else {
                    (ram.text(r, c) >> 8) as u8
                };
                ram.write_text(r, c, u16::from_be_bytes([attr, cpu.low(Slot::Accumulator)]));
            }
            cpu.set_carry(false);
        }
        0x12 if cpu.low(Slot::Base) == 0x10 => {
            cpu.set_wide(Slot::Base, 3); // color display, 256 KiB VGA memory
            cpu.set_wide(Slot::Count, 9);
            cpu.set_carry(false);
        }
        0x1a if cpu.low(Slot::Accumulator) == 0 => {
            cpu.set_low(Slot::Accumulator, 0x1a);
            cpu.set_wide(Slot::Base, 8);
            cpu.set_carry(false);
        }
        0x0E => {
            crate::bios::mscon::con_write_bytes(&[cpu.low(Slot::Accumulator)]);
            cpu.set_carry(false);
        }
        0x0F => {
            cpu.set_high(Slot::Accumulator, 80);
            cpu.set_low(Slot::Accumulator, ram.load(0x40, 0x49).unwrap_or(3));
            cpu.set_high(Slot::Base, 0);
            cpu.set_carry(false);
        }
        _ => cpu.set_carry(true),
    }
}

fn keyboard(cpu: &mut CpuState) {
    let function = cpu.high(Slot::Accumulator);
    match function {
        0 | 0x10 => {
            cpu.set_wide(Slot::Accumulator, crate::bios::pc::key(true).unwrap_or(0));
        }
        1 | 0x11 => {
            let key = crate::bios::pc::key(false);
            cpu.set_zero(key.is_none());
            cpu.set_wide(Slot::Accumulator, key.unwrap_or(0));
        }
        _ => {
            cpu.set_carry(true);
            return;
        }
    }
    cpu.set_carry(false);
}

fn serial_bios(cpu: &mut CpuState) {
    use crate::bios::pc;
    let Some(&base) = pc::UARTS.get(usize::from(cpu.wide(Slot::Data))) else {
        cpu.set_high(Slot::Accumulator, 0x80);
        return;
    };
    let function = cpu.high(Slot::Accumulator);
    let mut byte = cpu.low(Slot::Accumulator);
    let ok = match function {
        0 => {
            pc::configure_serial(base, byte);
            true
        }
        1 => pc::serial_write(base, byte),
        2 => {
            if let Some(read) = pc::serial_read(base) {
                byte = read;
                true
            } else {
                false
            }
        }
        3 => true,
        _ => false,
    };
    let status = pc::serial_status(base);
    cpu.set_wide(Slot::Accumulator, status);
    if matches!(function, 1 | 2) {
        cpu.set_low(Slot::Accumulator, byte);
    }
    if !ok {
        cpu.set_high(Slot::Accumulator, (status >> 8) as u8 | 0x80);
    }
}

fn printer_bios(cpu: &mut CpuState) {
    use crate::bios::pc;
    let ok = cpu.wide(Slot::Data) == 0
        && match cpu.high(Slot::Accumulator) {
            0 => pc::print_byte(cpu.low(Slot::Accumulator)),
            2 => true,
            _ => false,
        };
    cpu.set_high(Slot::Accumulator, pc::printer_status() | u8::from(!ok));
}

fn clock(cpu: &mut CpuState) {
    if cpu.high(Slot::Accumulator) == 0 {
        cpu.set_wide(Slot::Count, 0);
        cpu.set_wide(Slot::Data, 1);
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        cpu.set_carry(true);
    }
}

fn read_sectors(cpu: &mut CpuState, ram: &mut GuestRam) {
    if cpu.low(Slot::Data) == 0x80 {
        if !ide_read(cpu, ram) {
            cpu.set_high(Slot::Accumulator, 1);
            cpu.set_carry(true);
        }
        return;
    }
    if cpu.high(Slot::Accumulator) != 2 || !copy_sectors(cpu, ram) {
        cpu.set_high(Slot::Accumulator, 1);
        cpu.set_carry(true);
    }
}

/// INT 13h extended reads use a 16-byte disk address packet at DS:SI.
fn ide_read(cpu: &mut CpuState, ram: &mut GuestRam) -> bool {
    use crate::bios::ata;
    let Some(capacity) = ata::capacity() else {
        return false;
    };
    match cpu.high(Slot::Accumulator) {
        0x41 if cpu.wide(Slot::Base) == 0x55aa => {
            cpu.set_wide(Slot::Base, 0xaa55);
            cpu.set_wide(Slot::Count, 1);
            cpu.set_high(Slot::Accumulator, 0x30);
        }
        0x42 => {
            let ds = cpu.selector(Segment::Data);
            let si = cpu.wide(Slot::Source);
            let word = |at| read_word(ram, ds, si.wrapping_add(at)).unwrap();
            if word(0) != 16 || word(12) != 0 || word(14) != 0 {
                return false;
            }
            let (count, offset, segment) = (word(2), word(4), word(6));
            let first = u32::from(word(8)) | (u32::from(word(10)) << 16);
            if !(1..=127).contains(&count)
                || first.saturating_add(u32::from(count)) > capacity
                || u32::from(offset) + u32::from(count) * 512 > 65536
            {
                return false;
            }
            let mut sector = [0u8; 512];
            for index in 0..count {
                if !ata::read(first + u32::from(index), &mut sector) {
                    for (i, byte) in index.to_le_bytes().into_iter().enumerate() {
                        ram.store(ds, si.wrapping_add(2 + i as u16), byte);
                    }
                    return false;
                }
                let start = offset + index * 512;
                for (i, byte) in sector.iter().enumerate() {
                    ram.store(segment, start + i as u16, *byte);
                }
            }
            cpu.set_high(Slot::Accumulator, 0);
        }
        _ => return false,
    }
    cpu.set_carry(false);
    true
}

fn copy_sectors(cpu: &mut CpuState, ram: &mut GuestRam) -> bool {
    crate::guest::disk::with_media(|image| copy_chs(cpu, ram, image))
}

fn copy_chs(cpu: &mut CpuState, ram: &mut GuestRam, image: &[u8]) -> bool {
    if image.len() < 30 {
        return false;
    }
    let per_track = u16::from_le_bytes([image[24], image[25]]);
    let heads = u16::from_le_bytes([image[26], image[27]]);
    let count = cpu.low(Slot::Accumulator);
    if per_track == 0 || heads == 0 || count == 0 || cpu.low(Slot::Data) != 0 {
        return false;
    }
    let packed = cpu.low(Slot::Count);
    let sector = packed & 0x3F;
    if sector == 0 || u16::from(sector) > per_track || u16::from(cpu.high(Slot::Data)) >= heads {
        return false;
    }
    let cylinder = u32::from(cpu.high(Slot::Count)) | (u32::from(packed & 0xC0) << 2);
    let head = u32::from(cpu.high(Slot::Data));
    let linear =
        (cylinder * u32::from(heads) + head) * u32::from(per_track) + u32::from(sector - 1);
    let Some(start) = (linear as usize).checked_mul(512) else {
        return false;
    };
    let Some(bytes) = usize::from(count).checked_mul(512) else {
        return false;
    };
    let Some(end) = start.checked_add(bytes) else {
        return false;
    };
    let Some(data) = image.get(start..end) else {
        return false;
    };
    let segment = cpu.selector(Segment::Extra);
    let mut offset = cpu.wide(Slot::Base);
    for byte in data {
        if !ram.store(segment, offset, *byte) {
            return false;
        }
        offset = offset.wrapping_add(1);
    }
    cpu.set_low(Slot::Accumulator, count);
    cpu.set_high(Slot::Accumulator, 0);
    cpu.set_carry(false);
    true
}
