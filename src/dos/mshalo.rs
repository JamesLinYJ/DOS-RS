//! ROM cartridge scan from `DOS/MSHALO.ASM`.
//!
//! A machine id of `FD` at `F000:FFFE` starts a scan at segment `C000`.
//! Headers are the word `AA55`, every 2KB, and the scan stops at `F000`.
//! A name matches only when the byte after it is a space or `?`. The first
//! byte of the caller's name is not compared. A module length is rounded up
//! to a 2KB paragraph boundary before the next header.

#[derive(Clone, Copy)]
struct Span<'a> {
    linear: u32,
    bytes: &'a [u8],
}

const MACHINE_SEGMENT: u16 = 0xF000;
const MACHINE_OFFSET: u16 = 0xFFFE;
const MACHINE_MARK: u8 = 0xFD;
const SCAN_START: u16 = 0xC000;
const SCAN_LIMIT: u16 = 0xF000;
const SCAN_STEP: u16 = 0x80;
const HEADER: u16 = 0xAA55;
const NAME_LIST: u16 = 6;
const STACK_CAP: u16 = 0x1000;
const DMA_OFFSET: u16 = 0x80;

fn read_at(spans: &[Span<'_>], linear: u32) -> u8 {
    let mut index = 0;
    while index < spans.len() {
        let span = &spans[index];
        let end = span.linear + span.bytes.len() as u32;
        if linear >= span.linear && linear < end {
            return span.bytes[(linear - span.linear) as usize];
        }
        index += 1;
    }
    0
}

fn read_byte(spans: &[Span<'_>], segment: u16, offset: u16) -> u8 {
    read_at(spans, (segment as u32) * 16 + offset as u32)
}

fn read_word(spans: &[Span<'_>], segment: u16, offset: u16) -> u16 {
    let low = read_byte(spans, segment, offset) as u16;
    let high = read_byte(spans, segment, offset.wrapping_add(1)) as u16;
    low | (high << 8)
}

/// Paragraphs occupied by `blocks_512` units, rounded up to 2KB.
pub fn module_paragraphs(blocks_512: u8) -> u16 {
    let raw = (blocks_512 as u16) << 2;
    raw.wrapping_add(0x7F) & 0xFF80
}

/// Stack offset used before entering a cartridge.
pub fn rom_stack(paragraphs: u16) -> u16 {
    let bounded = if paragraphs < STACK_CAP { paragraphs } else { 0 };
    bounded << 4
}

/// Word placed in the cartridge's initial register image.
pub fn rom_initial_word() -> u16 {
    !0u16
}

/// Segment and entry offset of a matching cartridge name.
fn find_rom(spans: &[Span<'_>], name: &[u8]) -> Option<(u16, u16)> {
    if read_byte(spans, MACHINE_SEGMENT, MACHINE_OFFSET) != MACHINE_MARK {
        return None;
    }
    let mut segment = SCAN_START;
    loop {
        if read_word(spans, segment, 0) == HEADER {
            let skip = module_paragraphs(read_byte(spans, segment, 2));
            let mut cursor = NAME_LIST;
            loop {
                let length = read_byte(spans, segment, cursor) as u16;
                cursor = cursor.wrapping_add(1);
                if length == 0 {
                    segment = segment.wrapping_add(skip);
                    break;
                }
                let mut user_at = 1usize;
                let mut remaining = length;
                let mut same = true;
                while remaining != 0 {
                    let rom_byte = read_byte(spans, segment, cursor);
                    let user_byte = if user_at < name.len() { name[user_at] } else { 0 };
                    cursor = cursor.wrapping_add(1);
                    user_at += 1;
                    remaining -= 1;
                    if rom_byte != user_byte {
                        same = false;
                        cursor = cursor.wrapping_add(remaining);
                        break;
                    }
                }
                if same {
                    let tail = if user_at < name.len() { name[user_at] } else { 0 };
                    if tail == b'?' || tail == b' ' {
                        return Some((segment, cursor));
                    }
                }
                cursor = cursor.wrapping_add(3);
            }
        } else {
            segment = segment.wrapping_add(SCAN_STEP);
        }
        if segment >= SCAN_LIMIT {
            return None;
        }
    }
}

fn basic_header() -> [u8; 16] {
    let mut header = [0u8; 16];
    header[0] = 0x55;
    header[1] = 0xAA;
    header[2] = 1;
    header[6] = 5;
    header[7] = b'B';
    header[8] = b'A';
    header[9] = b'S';
    header[10] = b'I';
    header[11] = b'C';
    header
}

fn write_hit(hit: Option<(u16, u16)>, missing: &[u8]) {
    match hit {
        None => crate::bios::mscon::con_write_bytes(missing),
        Some((segment, entry)) => {
            crate::bios::mscon::con_write_bytes(b"HIT");
            put_hex16(segment);
            put_hex16(entry);
        }
    }
}

pub fn show() {
    let off = [0u8];
    let on = [0xFDu8];
    let module = basic_header();
    let basic = *b"\x05BASIC ";
    let rejected = *b"\x05BASICX";
    let question = *b"\x05BASIC?";
    let machine = Span {
        linear: (MACHINE_SEGMENT as u32) * 16 + MACHINE_OFFSET as u32,
        bytes: &on,
    };
    let at_start = Span {
        linear: (SCAN_START as u32) * 16,
        bytes: &module,
    };
    let at_next = Span {
        linear: (0xC080u32) * 16,
        bytes: &module,
    };
    let at_limit = Span {
        linear: (SCAN_LIMIT as u32) * 16,
        bytes: &module,
    };
    let no_machine = Span {
        linear: machine.linear,
        bytes: &off,
    };

    crate::bios::mscon::con_write_bytes(b"MSHALO ");
    write_hit(find_rom(&[no_machine, at_start], &basic), b"NO");
    crate::bios::mscon::con_write_bytes(b" ");
    write_hit(find_rom(&[machine, at_start], &basic), b"MISS");
    crate::bios::mscon::con_write_bytes(b" ");
    write_hit(find_rom(&[machine, at_start], &rejected), b"TERM");
    crate::bios::mscon::con_write_bytes(b" NEXT");
    write_hit(find_rom(&[machine, at_next], &basic), b"MISS");
    crate::bios::mscon::con_write_bytes(b" ");
    write_hit(find_rom(&[machine, at_limit], &basic), b"STOP");
    crate::bios::mscon::con_write_bytes(b" MARK");
    write_hit(find_rom(&[machine, at_start], &question), b"MISS");
    crate::bios::mscon::con_write_bytes(b" SK");
    put_hex16(module_paragraphs(1));
    crate::bios::mscon::con_write_bytes(b" SK");
    put_hex16(module_paragraphs(0x21));
    crate::bios::mscon::con_write_bytes(b" IW");
    put_hex16(rom_initial_word());
    crate::bios::mscon::con_write_bytes(b" DMA");
    put_hex16(DMA_OFFSET);
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex16(rom_stack(0));
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex16(rom_stack(0x0FFF));
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex16(rom_stack(0x0100));
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex16(rom_stack(STACK_CAP));
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
