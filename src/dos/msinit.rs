//! DOS initialization from `DOS/MSINIT.ASM`.
//!
//! This image is not the high-memory build and does not use the alternate
//! vector set. The installed build rewrites interrupt `2F`. The fake-version
//! list is walked by its length bytes. Swap sizes round up to an even count.
//! A block device raises the sector limit only when the new sector is larger.

const HIGH_MEMORY: bool = false;
const ALT_VECTORS: bool = false;
const INSTALLED: bool = true;
const FETCHI_TAG: u16 = 22642;
const LONG_JUMP: u8 = 0xEA;
const INT_BASE: usize = 0x80;
const ENTRY_POINT: usize = INT_BASE + 0x40;
const DEV_CHAR: u16 = 0x8000;
const CLOCK: u16 = 0x0008;
const CONSOLE_IN: u16 = 0x0001;
const CONSOLE_OUT: u16 = 0x0002;
const DPB_SIZE: u16 = 33;
const HANDLES: usize = 20;
const SHARER_SLOTS: usize = 15;
const ARENA_END: u8 = 0x5A;
const OWNER_SYSTEM: u16 = 0;
const FIRST_ACCESS: u8 = 0xFF;

struct Device<'a> {
    next: Option<usize>,
    attributes: u16,
    units: u8,
    sectors: &'a [u16],
}

struct DeviceWalk {
    console: u16,
    clock: u8,
    inits: u8,
    max_sector: u16,
    units: u8,
    skipped: u8,
}

fn dos_segment(current: u16, memory_end: u16, rounded: u16) -> u16 {
    if HIGH_MEMORY {
        memory_end.wrapping_sub(rounded)
    } else {
        current
    }
}

fn alt_segment_writes() -> u16 {
    if ALT_VECTORS { 15 } else { 0 }
}

fn round_even(length: u16) -> u16 {
    let carry = length & 1;
    let half = length >> 1;
    (half + carry) << 1
}

fn swap_lengths(start: u16, always: u16, end: u16) -> (u16, u16, u16) {
    let area = round_even(end.wrapping_sub(start));
    let in_dos = round_even(end.wrapping_sub(always));
    let always_len = always.wrapping_sub(start) | 0x8000;
    (area, in_dos, always_len)
}

fn paragraphs(offset: u16) -> u16 {
    let sum = offset as u32 + 15;
    let carry = u32::from(sum > 0xFFFF);
    let low = (sum as u16) as u32;
    let rotated = (low >> 1) | (carry << 15);
    (rotated >> 3) as u16
}

fn store_word(table: &mut [u8], at: usize, value: u16) {
    table[at] = value as u8;
    table[at + 1] = (value >> 8) as u8;
}

fn read_word(table: &[u8], at: usize) -> u16 {
    table[at] as u16 | ((table[at + 1] as u16) << 8)
}

fn fill_vectors(dos: u16, iret_offset: u16, int2f_offset: u16, call_offset: u16) -> [u8; 0x100] {
    let mut table = [0u8; 0x100];
    let mut cursor = INT_BASE + 2;
    let mut left = 17u16;
    while left != 0 {
        store_word(&mut table, cursor, dos);
        cursor += 2;
        left -= 1;
    }
    cursor += 6;
    left = 43;
    while left != 0 {
        store_word(&mut table, cursor, dos);
        cursor += 2;
        left -= 1;
    }
    cursor = INT_BASE;
    left = 9;
    while left != 0 {
        store_word(&mut table, cursor, iret_offset);
        cursor += 4;
        left -= 1;
    }
    cursor += 4;
    left = 22;
    while left != 0 {
        store_word(&mut table, cursor, iret_offset);
        cursor += 4;
        left -= 1;
    }
    if INSTALLED {
        store_word(&mut table, 0x2F * 4, int2f_offset);
    }
    table[ENTRY_POINT] = LONG_JUMP;
    store_word(&mut table, ENTRY_POINT + 1, call_offset);
    store_word(&mut table, ENTRY_POINT + 3, dos);
    table
}

fn push_entry(table: &mut [u8], cursor: &mut usize, name: &[u8], count: u8) {
    table[*cursor] = name.len() as u8;
    *cursor += 1;
    table[*cursor..*cursor + name.len()].copy_from_slice(name);
    *cursor += name.len();
    table[*cursor] = 3;
    table[*cursor + 1] = 40;
    table[*cursor + 2] = count;
    *cursor += 3;
}

fn fake_table() -> [u8; 151] {
    let mut table = [0u8; 151];
    let mut cursor = 0usize;
    push_entry(&mut table, &mut cursor, b"IBMCACHE.COM", 255);
    push_entry(&mut table, &mut cursor, b"IBMCACHE.SYS", 255);
    push_entry(&mut table, &mut cursor, b"DXMA0MOD.SYS", 255);
    push_entry(&mut table, &mut cursor, b"WIN200.BIN", 4);
    push_entry(&mut table, &mut cursor, b"PSCPG.COM", 255);
    push_entry(&mut table, &mut cursor, b"DCJSS02.EXE", 255);
    push_entry(&mut table, &mut cursor, b"ISAM.EXE", 255);
    push_entry(&mut table, &mut cursor, b"ISAM2.EXE", 255);
    push_entry(&mut table, &mut cursor, b"DFIA0MOD.SYS", 255);
    table
}

fn walk_fake(table: &[u8]) -> (u16, u8) {
    let mut cursor = 0usize;
    let mut entry = 0u8;
    let mut windows = 0u8;
    loop {
        let length = table[cursor] as usize;
        cursor += 1;
        if length == 0 {
            break;
        }
        let count = table[cursor + length + 2];
        if entry == 3 {
            windows = count;
        }
        cursor += length + 3;
        entry += 1;
    }
    (cursor as u16, windows)
}

fn handles() -> [u8; HANDLES] {
    let mut table = [0xFFu8; HANDLES];
    table[0] = 0;
    table[1] = 0;
    table[2] = 0;
    table
}

fn arena_size(memory_end: u16, pdb: u16) -> u16 {
    let head = pdb.wrapping_sub(1);
    let span = head.wrapping_sub(memory_end);
    span.wrapping_neg().wrapping_sub(1)
}

fn walk_devices(mut devices: [Device<'_>; 5], mut max_sector: u16) -> DeviceWalk {
    let mut inits = 1u8;
    devices[0].attributes |= CONSOLE_IN | CONSOLE_OUT;
    let console = devices[0].attributes;
    let mut index = 0usize;
    let clock = loop {
        index = devices[index].next.unwrap_or(index);
        inits += 1;
        if devices[index].attributes & CLOCK != 0 {
            break index;
        }
    };
    let mut units = 0u8;
    let mut skipped = 0u8;
    while let Some(next) = devices[index].next {
        index = next;
        inits += 1;
        if devices[index].attributes & DEV_CHAR != 0 {
            skipped += 1;
            continue;
        }
        let count = devices[index].units;
        units = units.wrapping_add(count);
        let mut unit = 0u8;
        while unit < count {
            let sector = devices[index].sectors[unit as usize];
            if sector > max_sector {
                max_sector = sector;
            }
            unit += 1;
        }
    }
    DeviceWalk {
        console,
        clock: clock as u8,
        inits,
        max_sector,
        units,
        skipped,
    }
}

fn moves_backward(destination: u16, source: u16) -> bool {
    destination > source
}

fn link_dpbs(base: u16, count: u16) -> (u16, u16) {
    let mut address = base;
    let mut links = [0u16; 8];
    let mut index = 0u16;
    while index < count {
        address = address.wrapping_add(DPB_SIZE);
        links[index as usize] = address;
        index += 1;
    }
    if count > 0 {
        links[(count - 1) as usize] = 0xFFFF;
    }
    (links[0], links[(count - 1) as usize])
}

fn sharer_tail(segment: u16) -> (u16, u16) {
    let mut words = [0u16; SHARER_SLOTS * 2];
    let mut slot = 0u16;
    while (slot as usize) < SHARER_SLOTS {
        words[(slot * 2) as usize] = slot;
        slot += 1;
    }
    let mut word = 0usize;
    let mut left = SHARER_SLOTS;
    while left != 0 {
        word += 1;
        words[word] = segment;
        word += 1;
        left -= 1;
    }
    (words[SHARER_SLOTS * 2 - 2], words[SHARER_SLOTS * 2 - 1])
}

pub fn show() {
    let table = fake_table();
    let (span, windows) = walk_fake(&table);
    let (area, in_dos, always) = swap_lengths(0, 5, 10);
    let segment = dos_segment(0x00AB, 0xA000, 0x0010);
    let vectors = fill_vectors(segment, 0x1111, 0x2222, 0x3333);
    let opened = handles();
    let mut unused = 0u8;
    let mut index = 0usize;
    while index < opened.len() {
        if opened[index] == 0xFF {
            unused += 1;
        }
        index += 1;
    }
    let disk_sectors = [512u16, 128, 513];
    let devices = [
        Device {
            next: Some(1),
            attributes: 0,
            units: 0,
            sectors: &[],
        },
        Device {
            next: Some(2),
            attributes: 0,
            units: 0,
            sectors: &[],
        },
        Device {
            next: Some(3),
            attributes: CLOCK,
            units: 0,
            sectors: &[],
        },
        Device {
            next: Some(4),
            attributes: DEV_CHAR,
            units: 0,
            sectors: &[],
        },
        Device {
            next: None,
            attributes: 0,
            units: 3,
            sectors: &disk_sectors,
        },
    ];
    let walked = walk_devices(devices, 512);
    let (first_link, last_link) = link_dpbs(0x200, 2);
    let (last_offset, last_segment) = sharer_tail(segment);

    crate::bios::mscon::con_write_bytes(b"MSINIT FT");
    put_hex16(span);
    crate::bios::mscon::con_write_bytes(b" WN");
    put_hex8(windows);
    crate::bios::mscon::con_write_bytes(b" TG");
    put_hex16(FETCHI_TAG);
    crate::bios::mscon::con_write_bytes(b" SW");
    put_hex16(area);
    crate::bios::mscon::con_write_bytes(b" ID");
    put_hex16(in_dos);
    crate::bios::mscon::con_write_bytes(b" AL");
    put_hex16(always);
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex16(paragraphs(0));
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex16(paragraphs(1));
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex16(paragraphs(16));
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex16(paragraphs(17));
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex16(paragraphs(0xFFF1));
    crate::bios::mscon::con_write_bytes(b" I21");
    put_hex16(read_word(&vectors, 0x21 * 4));
    put_hex16(read_word(&vectors, 0x21 * 4 + 2));
    crate::bios::mscon::con_write_bytes(b" I29");
    put_hex16(read_word(&vectors, 0x29 * 4));
    put_hex16(read_word(&vectors, 0x29 * 4 + 2));
    crate::bios::mscon::con_write_bytes(b" I2F");
    put_hex16(read_word(&vectors, 0x2F * 4));
    put_hex16(read_word(&vectors, 0x2F * 4 + 2));
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(vectors[ENTRY_POINT]);
    crate::bios::mscon::con_write_bytes(b" CE");
    put_hex16(read_word(&vectors, ENTRY_POINT + 1));
    crate::bios::mscon::con_write_bytes(b" SG");
    put_hex16(read_word(&vectors, ENTRY_POINT + 3));
    crate::bios::mscon::con_write_bytes(b" HM");
    put_hex8(u8::from(HIGH_MEMORY));
    crate::bios::mscon::con_write_bytes(b" AV");
    put_hex16(alt_segment_writes());
    crate::bios::mscon::con_write_bytes(b" JFN");
    put_hex8(opened[0]);
    crate::bios::mscon::con_write_bytes(b" N");
    put_hex8(unused);
    crate::bios::mscon::con_write_bytes(b" AR");
    put_hex8(ARENA_END);
    crate::bios::mscon::con_write_bytes(b" OW");
    put_hex16(OWNER_SYSTEM);
    crate::bios::mscon::con_write_bytes(b" SZ");
    put_hex16(arena_size(0xA000, 0x0100));
    crate::bios::mscon::con_write_bytes(b" CN");
    put_hex16(walked.console);
    crate::bios::mscon::con_write_bytes(b" CK");
    put_hex8(walked.clock);
    crate::bios::mscon::con_write_bytes(b" IN");
    put_hex8(walked.inits);
    crate::bios::mscon::con_write_bytes(b" MX");
    put_hex16(walked.max_sector);
    crate::bios::mscon::con_write_bytes(b" UN");
    put_hex8(walked.units);
    crate::bios::mscon::con_write_bytes(b" SK");
    put_hex8(walked.skipped);
    crate::bios::mscon::con_write_bytes(b" MV");
    put_hex8(u8::from(moves_backward(0x200, 0x100)));
    crate::bios::mscon::con_write_bytes(b" MV");
    put_hex8(u8::from(moves_backward(0x100, 0x100)));
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex16(first_link);
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex16(last_link);
    crate::bios::mscon::con_write_bytes(b" FA");
    put_hex8(FIRST_ACCESS);
    crate::bios::mscon::con_write_bytes(b" SH");
    put_hex16(last_segment);
    put_hex16(last_offset);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
