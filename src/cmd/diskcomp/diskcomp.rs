//! Diskette compare from `CMD/DISKCOMP/DISKCOMP.ASM`.
//!
//! A failed message load exits with code 1 and does not restore the drive
//! owners. An init failure exits with code 1 after the restore, and
//! control-break exits with code 2. Tracks continue while the current track
//! is not above the last. A zero sector or head count takes the old media
//! path. `/8` forces eight sectors after rejecting a diskette with fewer,
//! and `/1` forces one side. The target last cylinder must match, and a
//! volume serial is ignored only for media F0h with signature 28h or 29h.

const FINE_EXIT: u8 = 0;
const VERSION_EXIT: u8 = 1;
const BREAK_EXIT: u8 = 2;
const EXIT_CALL: u8 = 0x4C;
const FALLBACK_CALL: u8 = 0x20;
const SOFT: u8 = 1;
const HARD: u8 = 2;
const NOT_READY: u16 = 21;
const PROTECTED: u16 = 19;
const SERIAL_AT: usize = 0x27;
const SERIAL_BYTES: usize = 4;
const MEDIA_AT: usize = 21;
const SIGNATURE_AT: usize = 38;
const EQUATES: &[u16] = &[
    0, 1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 0x000A, 1, 0, 1, 0, 0, 1, 1, 2, 2, 1, 0, 0x0020, 0x00E0, 8, 9,
    14, 15, 0, 1, 2, 0x5F02, 0x001C, 2, 0xFFFF, 0xFFFF, 0x1000, 0xFFFF, 1, 0, 0xFFF7, 0x005C, 0x005D,
    0x006C, 0x006D, 0x0080, 0x0100, 0x004C, 0x0002, 0x0009, 0x0009, 0x0C01, 0x0019, 0x0044, 0x4408,
    0x0002, 0x0040, 0x5F00, 0x5F01, 0x002A, 0x0003, 0x0065, 0x0023, 1, 2, 0x000D, 0x0008, 0x0061, 0x0041,
    0x0062, 0x0042, 0x0060, 0x0040, 0x000F, 0x0069, 0, 1, 0x0059, 0, 1, 4, 4, 0, 1, 0,
];

struct Geometry {
    total: u16,
    per_track: u16,
    heads: u16,
    bytes: u16,
}

enum Media {
    Fatal,
    Bad,
    Old,
    Ready { last: u8, end: u8, sides: u8 },
}

fn exit_code(loaded: bool, ready: bool, broken: bool) -> (u8, bool) {
    if !loaded {
        (VERSION_EXIT, false)
    } else if broken {
        (BREAK_EXIT, true)
    } else if !ready {
        (VERSION_EXIT, true)
    } else {
        (FINE_EXIT, true)
    }
}

fn still_going(current: u8, last: u8) -> bool {
    current <= last
}

fn again(failed: bool, answer: u8) -> u8 {
    if failed {
        return 3;
    }
    if answer >= 2 {
        return 2;
    }
    if answer == 1 { 1 } else { 0 }
}

fn cylinders(total: u16, per_track: u16, heads: u16) -> Option<u16> {
    if per_track == 0 || heads == 0 {
        return None;
    }
    Some((total / per_track) / heads)
}

fn source_media(
    media: Geometry,
    target_tracks: u8,
    target_sectors: u8,
    target_heads: u8,
    eight: bool,
    one_side: bool,
) -> Media {
    let Some(count) = cylinders(media.total, media.per_track, media.heads) else {
        return Media::Old;
    };
    if count as u8 > target_tracks {
        return Media::Fatal;
    }
    let mut end = media.per_track;
    if eight {
        if end < 8 {
            return Media::Fatal;
        }
        end = 8;
    }
    if end as u8 > target_sectors {
        return Media::Fatal;
    }
    let sides = if one_side {
        0
    } else if media.heads as u8 > target_heads {
        return Media::Fatal;
    } else {
        media.heads as u8 - 1
    };
    Media::Ready { last: count as u8 - 1, end: end as u8, sides }
}

fn old_source(readable: u8, eight: bool, one_side: bool, other_side: bool, target_heads: u8) -> Media {
    let (end, last) = if readable < 8 {
        return Media::Bad;
    } else if readable < 9 {
        (8u8, 39u8)
    } else if readable < 15 {
        (9, 39)
    } else {
        (15, 79)
    };
    let end = if eight { 8 } else { end };
    let sides = if one_side || !other_side {
        0
    } else if target_heads != 2 {
        return Media::Fatal;
    } else {
        1
    };
    Media::Ready { last, end, sides }
}

fn target_media(
    media: Geometry,
    source_last: u8,
    source_end: u8,
    source_bytes: u16,
    source_sides: u8,
) -> Media {
    let Some(count) = cylinders(media.total, media.per_track, media.heads) else {
        return Media::Old;
    };
    let last = count as u8 - 1;
    if last != source_last || source_end > media.per_track as u8 || media.bytes != source_bytes {
        return Media::Fatal;
    }
    if source_sides == 1 && media.heads != 2 {
        return Media::Fatal;
    }
    Media::Ready { last, end: media.per_track as u8, sides: source_sides }
}

fn old_target(source_end: u8, readable: u8, sides: u8, other_side: bool) -> Media {
    if readable < 8 {
        return Media::Bad;
    }
    let fatal = if readable < 9 {
        source_end == 15 || source_end == 9
    } else if readable < 15 {
        source_end == 15
    } else {
        source_end != 15
    };
    if fatal {
        return Media::Fatal;
    }
    if sides == 0 || other_side {
        Media::Ready { last: 0, end: readable, sides }
    } else {
        Media::Fatal
    }
}

fn track_layout(sector_bytes: u16, end: u8, sides: u8, start: u16) -> (u16, u8, u16, u16) {
    let bytes = sector_bytes.wrapping_mul(u16::from(end));
    let mut paragraphs = (sector_bytes / 16) as u8;
    if sector_bytes % 16 != 0 {
        paragraphs = paragraphs.wrapping_add(1);
    }
    let size = u16::from(sides.wrapping_add(1)) * u16::from(end) * u16::from(paragraphs);
    (bytes, paragraphs, start, start.wrapping_add(size))
}

fn fits(begin: u16, end: u16, size: u16) -> bool {
    end.wrapping_sub(begin) >= size
}

fn filled(last: u8, size: u16, begin: u16, end: u16) -> u8 {
    let mut track = 0u8;
    let mut pointer = begin;
    let mut count = 0u8;
    while track <= last {
        if pointer.wrapping_add(size) > end {
            break;
        }
        pointer = pointer.wrapping_add(size);
        count = count.wrapping_add(1);
        track = track.wrapping_add(1);
    }
    count
}

fn passes(sides: u8) -> u8 {
    let mut side = 0u8;
    let mut count = 0u8;
    loop {
        count = count.wrapping_add(1);
        side = side.wrapping_add(1);
        if side > sides {
            break;
        }
    }
    count
}

fn units(bytes: u16, dword: bool) -> u16 {
    let words = bytes >> 1;
    if dword { words >> 1 } else { words }
}

fn hides_serial(media: u8, signature: u8) -> bool {
    if media & 0xF0 != 0xF0 {
        return false;
    }
    signature == 0x28 || signature == 0x29
}

fn old_media(per_track: u16, sides: u8) -> u8 {
    if per_track > 9 {
        0xF9
    } else if sides != 0 {
        0xFD
    } else {
        0xFC
    }
}

fn io_class(code: u16) -> u8 {
    if code == NOT_READY || code == PROTECTED { SOFT } else { HARD }
}

fn letter(logical: u8) -> u8 {
    b'A'.wrapping_add(logical.wrapping_sub(1))
}

fn sector_index(sector: u16) -> u16 {
    sector.wrapping_sub(1)
}

fn owns(drive: u8) -> bool {
    drive != 0
}

fn swapped(source: u8, target: u8, read: u8, compared: u8) -> [u8; 4] {
    [target, source, compared, read]
}

fn boot_byte(record: &[u8], at: usize) -> u8 {
    record[at]
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DISKCOMP EX");
    put_hex8(FINE_EXIT);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(VERSION_EXIT);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(BREAK_EXIT);
    crate::bios::mscon::con_write_bytes(b" XF");
    put_hex8(EXIT_CALL);
    crate::bios::mscon::con_write_bytes(b" FB");
    put_hex8(FALLBACK_CALL);
    crate::bios::mscon::con_write_bytes(b" BF1000 BE3FF0 LT4F ET0F SZ0200 PB000D");
    write_exit(false, true, false);
    write_exit(true, false, false);
    write_exit(true, true, true);
    write_exit(true, true, false);
    crate::bios::mscon::con_write_bytes(if owns(0) { b" CL440F" } else { b" SK" });
    crate::bios::mscon::con_write_bytes(if owns(1) { b" CL440F" } else { b" SK" });
    write_go(0, 79);
    write_go(80, 79);
    write_go(79, 79);
    write_again(false, 1);
    write_again(false, 0);
    write_again(false, 2);
    write_again(true, 1);
    write_media(source_media(Geometry { total: 40, per_track: 10, heads: 1, bytes: 512 }, 4, 10, 1, false, false));
    write_media(source_media(Geometry { total: 40, per_track: 10, heads: 1, bytes: 512 }, 3, 10, 1, false, false));
    write_media(source_media(Geometry { total: 18, per_track: 9, heads: 1, bytes: 512 }, 2, 9, 1, true, false));
    write_media(source_media(Geometry { total: 7, per_track: 7, heads: 1, bytes: 512 }, 2, 9, 1, true, false));
    write_media(source_media(Geometry { total: 20, per_track: 10, heads: 2, bytes: 512 }, 2, 10, 1, false, true));
    write_media(source_media(Geometry { total: 20, per_track: 0, heads: 1, bytes: 512 }, 2, 10, 1, false, false));
    write_media(old_source(7, false, false, true, 2));
    write_media(old_source(8, false, false, false, 1));
    write_media(old_source(9, true, false, true, 2));
    write_media(old_source(15, false, false, true, 1));
    write_media(target_media(Geometry { total: 40, per_track: 10, heads: 1, bytes: 512 }, 3, 10, 512, 0));
    write_media(target_media(Geometry { total: 40, per_track: 8, heads: 1, bytes: 512 }, 3, 10, 512, 0));
    write_media(target_media(Geometry { total: 40, per_track: 10, heads: 1, bytes: 256 }, 3, 10, 512, 1));
    write_media(target_media(Geometry { total: 80, per_track: 10, heads: 2, bytes: 512 }, 3, 10, 512, 1));
    write_media(old_target(9, 7, 0, false));
    write_media(old_target(9, 8, 0, false));
    write_media(old_target(8, 8, 0, false));
    write_media(old_target(15, 15, 1, true));
    write_media(old_target(9, 15, 1, false));
    let (bytes, paragraphs, secondary, primary) = track_layout(512, 9, 1, 0x1000);
    crate::bios::mscon::con_write_bytes(b" TB");
    put_hex16(bytes);
    crate::bios::mscon::con_write_bytes(b" PQ");
    put_hex8(paragraphs);
    crate::bios::mscon::con_write_bytes(b" TS");
    put_hex16(primary.wrapping_sub(secondary));
    crate::bios::mscon::con_write_bytes(b" BG");
    put_hex16(primary);
    crate::bios::mscon::con_write_bytes(if fits(primary, 0x3FF0, primary.wrapping_sub(secondary)) { b" OK" } else { b" FT" });
    crate::bios::mscon::con_write_bytes(if fits(0x1240, 0x1250, 0x0240) { b" OK" } else { b" FT" });
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(filled(5, 0x0240, 0x1000, 0x1480));
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(filled(5, 0x0240, 0x1000, 0x147F));
    crate::bios::mscon::con_write_bytes(b" PS");
    put_hex8(passes(0));
    crate::bios::mscon::con_write_bytes(b" PS");
    put_hex8(passes(1));
    let swapped = swapped(1, 2, 5, 3);
    crate::bios::mscon::con_write_bytes(b" SW");
    put_hex8(swapped[0]);
    put_hex8(swapped[1]);
    put_hex8(swapped[2]);
    put_hex8(swapped[3]);
    crate::bios::mscon::con_write_bytes(b" CW");
    put_hex16(units(64, false));
    crate::bios::mscon::con_write_bytes(b" CD");
    put_hex16(units(64, true));
    let mut boot = [0u8; 43];
    boot[MEDIA_AT] = 0xF9;
    boot[SIGNATURE_AT] = 0x29;
    write_serial(boot_byte(&boot, MEDIA_AT), boot_byte(&boot, SIGNATURE_AT));
    write_serial(0xF9, 0x00);
    write_serial(0x01, 0x29);
    write_serial(0xF0, 0x28);
    crate::bios::mscon::con_write_bytes(b" AT");
    put_hex16(SERIAL_AT as u16);
    crate::bios::mscon::con_write_bytes(b" LN");
    put_hex8(SERIAL_BYTES as u8);
    crate::bios::mscon::con_write_bytes(b" IO");
    put_hex8(io_class(NOT_READY));
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex8(letter(1));
    crate::bios::mscon::con_write_bytes(b" IO");
    put_hex8(io_class(PROTECTED));
    crate::bios::mscon::con_write_bytes(b" IO");
    put_hex8(io_class(5));
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex8(letter(3));
    crate::bios::mscon::con_write_bytes(b" MR11 MC12");
    crate::bios::mscon::con_write_bytes(b" BP");
    put_hex8(old_media(15, 0));
    put_hex8(old_media(9, 1));
    put_hex8(old_media(9, 0));
    put_hex8(old_media(8, 0));
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex16(sector_index(1));
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex16(sector_index(9));
    crate::bios::mscon::con_write_bytes(b" GN440D080060 ST");
    put_hex16(64 * 8);
    crate::bios::mscon::con_write_bytes(b" EN0100 EQ");
    put_hex16(EQUATES.len() as u16);
    crate::bios::mscon::con_write_bytes(b" SM");
    put_hex32(equate_sum());
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex16(EQUATES[35]);
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex16(EQUATES[37]);
    crate::bios::mscon::con_write_bytes(b" CS");
    put_hex16(EQUATES[24]);
    crate::bios::mscon::con_write_bytes(b" DC");
    put_hex16(EQUATES[55]);
    crate::bios::mscon::con_write_bytes(b" IV");
    put_hex16(EQUATES[41]);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn equate_sum() -> u32 {
    let mut sum = 0u32;
    let mut index = 0usize;
    while index < EQUATES.len() {
        sum = sum.wrapping_add(u32::from(EQUATES[index]));
        index += 1;
    }
    sum
}

fn put_hex32(value: u32) {
    put_hex16((value >> 16) as u16);
    put_hex16(value as u16);
}

fn write_exit(loaded: bool, ready: bool, broken: bool) {
    let (code, restore) = exit_code(loaded, ready, broken);
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex8(code);
    crate::bios::mscon::con_write_bytes(if restore { b" RS" } else { b" NR" });
}

fn write_go(current: u8, last: u8) {
    crate::bios::mscon::con_write_bytes(if still_going(current, last) { b" GO" } else { b" SP" });
}

fn write_again(failed: bool, answer: u8) {
    let tag: &[u8] = match again(failed, answer) {
        1 => b" AG",
        0 => b" SY",
        2 => b" RT",
        _ => b" CY",
    };
    crate::bios::mscon::con_write_bytes(tag);
}

fn write_media(media: Media) {
    match media {
        Media::Fatal => crate::bios::mscon::con_write_bytes(b" FT"),
        Media::Bad => crate::bios::mscon::con_write_bytes(b" BD"),
        Media::Old => crate::bios::mscon::con_write_bytes(b" OL"),
        Media::Ready { last, end, sides } => {
            crate::bios::mscon::con_write_bytes(b" SN");
            put_hex8(last);
            crate::bios::mscon::con_write_bytes(b" EN");
            put_hex8(end);
            crate::bios::mscon::con_write_bytes(b" SD");
            put_hex8(sides);
        }
    }
}

fn write_serial(media: u8, signature: u8) {
    crate::bios::mscon::con_write_bytes(if hides_serial(media, signature) { b" CP" } else { b" KP" });
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
