//! BIOS interrupt adapter for emulated CPUs.
use super::*;

pub(super) fn serve_bios(cpu: &mut CpuState, ram: &mut GuestRam, vector: u8) -> bool {
    match vector {
        0x10 => video(cpu),
        0x13 => read_sectors(cpu, ram),
        0x14 => serial_bios(cpu),
        0x17 => printer_bios(cpu),
        0x16 => keyboard(cpu),
        0x1A => clock(cpu),
        _ => return false,
    }
    true
}

fn video(cpu: &mut CpuState) {
    match cpu.high(Slot::Accumulator) {
        0x0E => {
            crate::bios::mscon::con_write_bytes(&[cpu.low(Slot::Accumulator)]);
            cpu.set_carry(false);
        }
        0x0F => {
            cpu.set_high(Slot::Accumulator, 80);
            cpu.set_low(Slot::Accumulator, 3);
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
