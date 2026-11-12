//! DOS file-control-block adapter over the mounted volume.
use super::*;

fn load_fcb(ram: &GuestRam, segment: u16, offset: u16) -> Option<[u8; 37]> {
    let mut raw = [0u8; 37];
    for (index, byte) in raw.iter_mut().enumerate() {
        *byte = ram.load(segment, offset.wrapping_add(index as u16))?;
    }
    Some(raw)
}

fn fcb_name(raw: &[u8; 37]) -> [u8; 11] {
    let mut name = [b' '; 11];
    name.copy_from_slice(&raw[1..12]);
    name
}

pub(super) fn fcb_find(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session, continued: bool) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    if raw[0] > 1 {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let start = if continued {
        u16::from_le_bytes([raw[0x18], raw[0x19]])
    } else {
        0
    };
    let Some(found) = crate::guest::disk::find_match(
        crate::guest::disk::working_directory(),
        &fcb_name(&raw),
        0,
        start,
    ) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let next_index = found.index.saturating_add(1).to_le_bytes();
    if !ram.store(segment, offset.wrapping_add(0x18), next_index[0])
        || !ram.store(segment, offset.wrapping_add(0x19), next_index[1])
        || !write_found_fcb(ram, session, &found)
    {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

fn write_found_fcb(
    ram: &mut GuestRam,
    session: &Session,
    found: &crate::guest::disk::DirMatch,
) -> bool {
    let (segment, offset) = dta_target(session);
    let size = found.size.to_le_bytes();
    let time = found.time.to_le_bytes();
    let date = found.date.to_le_bytes();
    let mut raw = [0u8; 33];
    raw[0] = 1;
    raw[1..12].copy_from_slice(&found.name);
    raw[0x0E] = 128;
    raw[0x10..0x14].copy_from_slice(&size);
    raw[0x14..0x16].copy_from_slice(&date);
    raw[0x16..0x18].copy_from_slice(&time);
    for (index, byte) in raw.iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(index as u16), *byte) {
            return false;
        }
    }
    true
}

pub(super) fn fcb_delete(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    if crate::guest::disk::remove_file(&fcb_name(&raw)) {
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        cpu.set_low(Slot::Accumulator, 0xFF);
    }
}

pub(super) fn fcb_open(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = crate::guest::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let (time, date) = crate::guest::disk::stamp(&name).unwrap_or((0, 0));
    if raw[0] == 0 && !ram.store(segment, offset, 1) {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let size = (len as u32).to_le_bytes();
    let time_bytes = time.to_le_bytes();
    let date_bytes = date.to_le_bytes();
    let fields = [
        (0x0C, 0),
        (0x0D, 0),
        (0x0E, 128),
        (0x0F, 0),
        (0x10, size[0]),
        (0x11, size[1]),
        (0x12, size[2]),
        (0x13, size[3]),
        (0x14, date_bytes[0]),
        (0x15, date_bytes[1]),
        (0x16, time_bytes[0]),
        (0x17, time_bytes[1]),
        (0x20, 0),
    ];
    for (at, byte) in fields {
        if !ram.store(segment, offset.wrapping_add(at), byte) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            return;
        }
    }
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

pub(super) fn fcb_close(cpu: &mut CpuState, ram: &GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let size = u32::from_le_bytes([raw[0x10], raw[0x11], raw[0x12], raw[0x13]]);
    let time = u16::from_le_bytes([raw[0x16], raw[0x17]]);
    let date = u16::from_le_bytes([raw[0x14], raw[0x15]]);
    if crate::guest::disk::finish_fcb(&fcb_name(&raw), size, time, date) {
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        cpu.set_low(Slot::Accumulator, 0xFF);
    }
}

pub(super) fn fcb_read(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = crate::guest::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let mut record = u32::from(u16::from_le_bytes([raw[0x0E], raw[0x0F]]));
    if record == 0 {
        record = 128;
    }
    let block = u32::from(u16::from_le_bytes([raw[0x0C], raw[0x0D]]));
    let current = u32::from(raw[0x20]);
    let index = block.saturating_mul(128).saturating_add(current);
    let start = index.saturating_mul(record);
    if start >= len as u32 {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let take = record.min((len as u32) - start);
    if start >= 128 || start + take > 128 {
        cpu.set_low(Slot::Accumulator, 2);
        return;
    }
    let mut file = [0u8; 128];
    if crate::guest::disk::read_file(&name, &mut file).is_none() {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let (dta_segment, dta_offset) = dta_target(session);
    let mut copied = 0u32;
    while copied < record {
        let byte = if copied < take {
            file[(start + copied) as usize]
        } else {
            0
        };
        if !ram.store(dta_segment, dta_offset.wrapping_add(copied as u16), byte) {
            cpu.set_low(Slot::Accumulator, 2);
            return;
        }
        copied += 1;
    }
    let next = current + 1;
    let (block_next, record_next) = if next >= 128 {
        (block + 1, 0u8)
    } else {
        (block, next as u8)
    };
    let block_bytes = (block_next as u16).to_le_bytes();
    if !ram.store(segment, offset.wrapping_add(0x0C), block_bytes[0])
        || !ram.store(segment, offset.wrapping_add(0x0D), block_bytes[1])
        || !ram.store(segment, offset.wrapping_add(0x20), record_next)
    {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    cpu.set_low(Slot::Accumulator, if take < record { 3 } else { 0 });
    cpu.set_carry(false);
}

pub(super) fn fcb_write(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = crate::guest::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let mut record = u32::from(u16::from_le_bytes([raw[0x0E], raw[0x0F]]));
    if record == 0 {
        record = 128;
    }
    let block = u32::from(u16::from_le_bytes([raw[0x0C], raw[0x0D]]));
    let current = u32::from(raw[0x20]);
    let index = block.saturating_mul(128).saturating_add(current);
    let start = index.saturating_mul(record);
    let end = start.saturating_add(record);
    if record > 128 || len > 128 || end > 128 {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let mut file = [0u8; 128];
    if len > 0 && crate::guest::disk::read_file(&name, &mut file[..len]).is_none() {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let (dta_segment, dta_offset) = dta_target(session);
    let mut copied = 0u32;
    while copied < record {
        let Some(byte) = ram.load(dta_segment, dta_offset.wrapping_add(copied as u16)) else {
            cpu.set_low(Slot::Accumulator, 2);
            return;
        };
        file[(start + copied) as usize] = byte;
        copied += 1;
    }
    let new_len = (len as u32).max(end) as usize;
    if !crate::guest::disk::write_file(&name, &file[..new_len]) {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let size = (new_len as u32).to_le_bytes();
    for (index, byte) in size.into_iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(0x10 + index as u16), byte) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            return;
        }
    }
    let next = current + 1;
    let (block_next, record_next) = if next >= 128 {
        (block + 1, 0u8)
    } else {
        (block, next as u8)
    };
    let block_bytes = (block_next as u16).to_le_bytes();
    if !ram.store(segment, offset.wrapping_add(0x0C), block_bytes[0])
        || !ram.store(segment, offset.wrapping_add(0x0D), block_bytes[1])
        || !ram.store(segment, offset.wrapping_add(0x20), record_next)
    {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

pub(super) fn fcb_create(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    if !crate::guest::disk::write_named(&fcb_name(&raw), b"", 0x20) {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    fcb_open(cpu, ram);
}

pub(super) fn fcb_rename(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let mut to = [b' '; 11];
    to.copy_from_slice(&raw[0x12..0x1D]);
    if crate::guest::disk::rename_file(&fcb_name(&raw), &to) {
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        cpu.set_low(Slot::Accumulator, 0xFF);
    }
}

pub(super) fn fcb_random_read(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = crate::guest::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let record = fcb_record(&raw);
    let start = random_index(&raw, record).saturating_mul(record);
    if start >= len as u32 {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let take = record.min((len as u32) - start);
    if start >= 128 || start + take > 128 {
        cpu.set_low(Slot::Accumulator, 2);
        return;
    }
    let mut file = [0u8; 128];
    if crate::guest::disk::read_file(&name, &mut file).is_none() {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let (dta_segment, dta_offset) = dta_target(session);
    let mut copied = 0u32;
    while copied < record {
        let byte = if copied < take {
            file[(start + copied) as usize]
        } else {
            0
        };
        if !ram.store(dta_segment, dta_offset.wrapping_add(copied as u16), byte) {
            cpu.set_low(Slot::Accumulator, 2);
            return;
        }
        copied += 1;
    }
    cpu.set_low(Slot::Accumulator, if take < record { 3 } else { 0 });
    cpu.set_carry(false);
}

pub(super) fn fcb_random_write(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = crate::guest::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let record = fcb_record(&raw);
    let start = random_index(&raw, record).saturating_mul(record);
    let end = start.saturating_add(record);
    if record > 128 || len > 128 || end > 128 {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let mut file = [0u8; 128];
    if len > 0 && crate::guest::disk::read_file(&name, &mut file[..len]).is_none() {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let (dta_segment, dta_offset) = dta_target(session);
    let mut copied = 0u32;
    while copied < record {
        let Some(byte) = ram.load(dta_segment, dta_offset.wrapping_add(copied as u16)) else {
            cpu.set_low(Slot::Accumulator, 2);
            return;
        };
        file[(start + copied) as usize] = byte;
        copied += 1;
    }
    let new_len = (len as u32).max(end) as usize;
    if !crate::guest::disk::write_file(&name, &file[..new_len]) {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let size = (new_len as u32).to_le_bytes();
    for (index, byte) in size.into_iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(0x10 + index as u16), byte) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            return;
        }
    }
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

pub(super) fn fcb_block_read(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = crate::guest::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    };
    let record = fcb_record(&raw);
    if record > 128 || len > 128 {
        cpu.set_low(Slot::Accumulator, 2);
        cpu.set_carry(false);
        return;
    }
    let mut file = [0u8; 128];
    if len > 0 && crate::guest::disk::read_file(&name, &mut file[..len]).is_none() {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    }
    let mut index = random_index(&raw, record);
    let want = cpu.wide(Slot::Count);
    let (dta_segment, mut dta_offset) = dta_target(session);
    let mut done = 0u16;
    let mut status = 0u8;
    while done < want {
        let start = index.saturating_mul(record);
        if start >= len as u32 {
            status = 1;
            break;
        }
        let take = record.min((len as u32) - start);
        if start.saturating_add(record) > 128 {
            status = 2;
            break;
        }
        let mut copied = 0u32;
        let mut stored = true;
        while copied < record {
            let byte = if copied < take {
                file[(start + copied) as usize]
            } else {
                0
            };
            if !ram.store(dta_segment, dta_offset.wrapping_add(copied as u16), byte) {
                stored = false;
                break;
            }
            copied += 1;
        }
        if !stored {
            status = 2;
            break;
        }
        dta_offset = dta_offset.wrapping_add(record as u16);
        index = index.saturating_add(1);
        done = done.saturating_add(1);
        if take < record {
            status = 3;
            break;
        }
    }
    let mut bytes = index.to_le_bytes();
    if record >= 64 {
        bytes[3] = 0;
    }
    for (at, byte) in bytes.into_iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(0x21 + at as u16), byte) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            cpu.set_carry(true);
            return;
        }
    }
    let block = u16::try_from(index / 128).unwrap_or(u16::MAX);
    let block_bytes = block.to_le_bytes();
    let current = u8::try_from(index % 128).unwrap_or(u8::MAX);
    if !ram.store(segment, offset.wrapping_add(0x0C), block_bytes[0])
        || !ram.store(segment, offset.wrapping_add(0x0D), block_bytes[1])
        || !ram.store(segment, offset.wrapping_add(0x20), current)
    {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    }
    cpu.set_wide(Slot::Count, done);
    cpu.set_low(Slot::Accumulator, status);
    cpu.set_carry(false);
}

pub(super) fn fcb_block_write(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = crate::guest::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    };
    let record = fcb_record(&raw);
    if record > 128 || len > 128 {
        cpu.set_low(Slot::Accumulator, 1);
        cpu.set_carry(false);
        return;
    }
    let mut file = [0u8; 128];
    if len > 0 && crate::guest::disk::read_file(&name, &mut file[..len]).is_none() {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    }
    let mut index = random_index(&raw, record);
    let want = cpu.wide(Slot::Count);
    let (dta_segment, mut dta_offset) = dta_target(session);
    let mut done = 0u16;
    let mut status = 0u8;
    while done < want {
        let start = index.saturating_mul(record);
        if start.saturating_add(record) > 128 {
            status = 1;
            break;
        }
        let mut piece = [0u8; 128];
        let mut copied = 0u32;
        let mut loaded = true;
        while copied < record {
            let Some(byte) = ram.load(dta_segment, dta_offset.wrapping_add(copied as u16)) else {
                loaded = false;
                break;
            };
            piece[copied as usize] = byte;
            copied += 1;
        }
        if !loaded {
            status = 2;
            break;
        }
        let mut placed = 0u32;
        while placed < record {
            file[(start + placed) as usize] = piece[placed as usize];
            placed += 1;
        }
        dta_offset = dta_offset.wrapping_add(record as u16);
        index = index.saturating_add(1);
        done = done.saturating_add(1);
    }
    let end = index.saturating_mul(record);
    let Ok(old) = u32::try_from(len) else {
        cpu.set_low(Slot::Accumulator, 1);
        cpu.set_carry(false);
        return;
    };
    let new_len = old.max(end);
    if done > 0 {
        let Ok(bytes_len) = usize::try_from(new_len) else {
            cpu.set_low(Slot::Accumulator, 1);
            cpu.set_carry(false);
            return;
        };
        if bytes_len > file.len() || !crate::guest::disk::write_file(&name, &file[..bytes_len]) {
            cpu.set_low(Slot::Accumulator, 1);
            cpu.set_wide(Slot::Count, 0);
            cpu.set_carry(false);
            return;
        }
        let size_bytes = new_len.to_le_bytes();
        for (at, byte) in size_bytes.into_iter().enumerate() {
            if !ram.store(segment, offset.wrapping_add(0x10 + at as u16), byte) {
                cpu.set_low(Slot::Accumulator, 0xFF);
                cpu.set_carry(true);
                return;
            }
        }
    }
    let mut place = index.to_le_bytes();
    if record >= 64 {
        place[3] = 0;
    }
    for (at, byte) in place.into_iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(0x21 + at as u16), byte) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            cpu.set_carry(true);
            return;
        }
    }
    let block = u16::try_from(index / 128).unwrap_or(u16::MAX);
    let block_bytes = block.to_le_bytes();
    let current = u8::try_from(index % 128).unwrap_or(u8::MAX);
    if !ram.store(segment, offset.wrapping_add(0x0C), block_bytes[0])
        || !ram.store(segment, offset.wrapping_add(0x0D), block_bytes[1])
        || !ram.store(segment, offset.wrapping_add(0x20), current)
    {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    }
    cpu.set_wide(Slot::Count, done);
    cpu.set_low(Slot::Accumulator, status);
    cpu.set_carry(false);
}

pub(super) fn fcb_place(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        return;
    };
    let block = u32::from(u16::from_le_bytes([raw[0x0C], raw[0x0D]]));
    let index = block
        .saturating_mul(128)
        .saturating_add(u32::from(raw[0x20]));
    let bytes = index.to_le_bytes();
    for (index, byte) in bytes.into_iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(0x21 + index as u16), byte) {
            return;
        }
    }
    cpu.set_carry(false);
}

fn fcb_record(raw: &[u8; 37]) -> u32 {
    let record = u32::from(u16::from_le_bytes([raw[0x0E], raw[0x0F]]));
    if record == 0 { 128 } else { record }
}

fn random_index(raw: &[u8; 37], record: u32) -> u32 {
    u32::from_le_bytes([
        raw[0x21],
        raw[0x22],
        raw[0x23],
        if record < 64 { raw[0x24] } else { 0 },
    ])
}

pub(super) fn fcb_size(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let mut raw = [0u8; 37];
    for (index, byte) in raw.iter_mut().enumerate() {
        let Some(loaded) = ram.load(segment, offset.wrapping_add(index as u16)) else {
            cpu.set_low(Slot::Accumulator, 0xFF);
            return;
        };
        *byte = loaded;
    }
    let mut name = [b' '; 11];
    name.copy_from_slice(&raw[1..12]);
    let mut record = u32::from(u16::from_le_bytes([raw[0x0E], raw[0x0F]]));
    if record == 0 {
        record = 128;
    }
    let Some(len) = crate::guest::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let records = (len as u32).div_ceil(record);
    for (index, byte) in records.to_le_bytes().into_iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(0x21 + index as u16), byte) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            return;
        }
    }
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}
