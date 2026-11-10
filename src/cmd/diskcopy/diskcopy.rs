//! Diskette copy from `CMD/DISKCOPY/DISKCOPY.ASM`.
//!
//! A failed message load exits with code 1 and does not restore the drive
//! owners. An init failure exits with code 1 after the restore, and
//! control-break exits with code 2. Tracks continue while the track being
//! written is not above the last. There is no `/8` switch. `/1` forces one
//! side. A target whose cylinder count differs is formatted, except a
//! 15-sector target when the source is a 40-track diskette. A boot serial is
//! replaced only for media `F0h` with signature 28h or 29h.

const FINE_EXIT: u8 = 0;
const VERSION_EXIT: u8 = 1;
const BREAK_EXIT: u8 = 2;
const PARSE_EXIT: u8 = 1;
const EXIT_CALL: u8 = 0x4C;
const FALLBACK_CALL: u8 = 0x20;
const NO_ERROR: u8 = 0;
const SOFT: u8 = 1;
const HARD: u8 = 2;
const NOT_READY: u16 = 21;
const PROTECTED: u16 = 19;
const UNKNOWN_MEDIA: u16 = 26;
const READ_TRACK: u8 = 0x61;
const WRITE_TRACK: u8 = 0x41;
const BUFFER_BEGIN: u16 = 0x1000;
const BUFFER_END: u16 = 0x3FF0;
const LAST_TRACK: u8 = 79;
const END_OF_TRACK: u8 = 15;
const SECTOR_BYTES: u16 = 512;
const STACK_COPIES: u16 = 64;
const STACK_ENTRY: u16 = 8;
const BPB_BYTES: u16 = 13;
const SINGLE_DRIVE: u8 = 1;
const DOUBLE_DRIVE: u8 = 2;
const SET_BEFORE_RETURN: u8 = 4;
const SET_BEFORE_FORMAT: u8 = 5;
const FORMAT_SPECIAL: u8 = 0;
const ROM_REJECTED: u8 = 2;
const IOCTL_CALL: u16 = 0x440D;
const MAJOR: u8 = 0x08;
const SELECT_PRESENT: u8 = 0xFF;
const SELECT_CALL: u16 = 0xADC1;
const YESNO_CALL: u16 = 0x6523;
const FORTY_TRACKS: u8 = 0x27;
const SERIAL_LOW_AT: u16 = 0;
const SERIAL_HIGH_AT: u16 = 2;

struct Geometry {
    total: u16,
    per_track: u16,
    heads: u16,
    bytes: u16,
}

struct ErrorScene {
    code: u16,
    formatting: bool,
    trying: bool,
    on_target: bool,
    timed_out: bool,
    soft_while_trying: bool,
    drive: u8,
}

enum Media {
    Fatal,
    Bad,
    Old,
    Ready { last: u8, end: u8, sides: u8 },
}

enum Probe {
    Failed,
    Eight,
    Nine,
    Fifteen,
}

enum Target {
    Fatal,
    Format,
    Ready,
    Old,
}

enum ReadEnd {
    Adjusted,
    Clean,
    Again,
    Reported,
    Silent,
}

enum WriteEnd {
    Clean,
    Again,
    GaveUp,
    FormatFatal,
    FormatRetry,
}

enum FormatEnd {
    Compatible,
    TargetBad,
    Finished(u8),
}

enum IoFate {
    Soft { letter: Option<u8> },
    Hard { unknown: bool },
    TimedOut,
    Kept,
}

struct CopyEnd {
    unusable: bool,
    fatal: bool,
    volser: bool,
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

fn owns(drive: u8) -> bool {
    drive != 0
}

fn parm_restore(flag: u8) -> Option<u8> {
    if flag != 0 { Some(SET_BEFORE_RETURN) } else { None }
}

fn prompts_once(copy_type: u8) -> bool {
    copy_type == DOUBLE_DRIVE
}

fn copy_end(read_fatal: bool, write_fatal: bool, errors: u8) -> CopyEnd {
    if read_fatal || write_fatal {
        CopyEnd { unusable: false, fatal: true, volser: false }
    } else {
        CopyEnd { unusable: errors != 0, fatal: false, volser: true }
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

fn may_fill(track: u8, fatal: bool) -> bool {
    track != 0 || !fatal
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

fn written(last: u8, size: u16, begin: u16, end: u16, fatal_at: Option<u8>) -> u8 {
    let mut track = 0u8;
    let mut pointer = begin;
    let mut count = 0u8;
    while track <= last {
        if pointer.wrapping_add(size) > end || fatal_at == Some(track) {
            break;
        }
        pointer = pointer.wrapping_add(size);
        count = count.wrapping_add(1);
        track = track.wrapping_add(1);
    }
    count
}

fn passes(sides: u8, stop_early: bool) -> u8 {
    let mut side = 0u8;
    let mut count = 0u8;
    loop {
        count = count.wrapping_add(1);
        if stop_early {
            break;
        }
        side = side.wrapping_add(1);
        if side > sides {
            break;
        }
    }
    count
}

fn read_end(error: u8, cylinder: u8, head: u8, show_message: bool) -> ReadEnd {
    if error == NO_ERROR {
        if cylinder == 0 && head == 0 { ReadEnd::Adjusted } else { ReadEnd::Clean }
    } else if error == SOFT {
        ReadEnd::Again
    } else if show_message {
        ReadEnd::Reported
    } else {
        ReadEnd::Silent
    }
}

fn hashed(
    media: u8,
    signature: u8,
    date_year: u16,
    date_day: u16,
    time_clock: u16,
    time_day: u16,
) -> Option<(u16, u16)> {
    if media & 0xF0 != 0xF0 || (signature != 0x28 && signature != 0x29) {
        return None;
    }
    Some((date_day.wrapping_add(time_day), date_year.wrapping_add(time_clock)))
}

fn write_end(error: u8, formatted: bool, format_fatal: bool) -> WriteEnd {
    if error == NO_ERROR {
        WriteEnd::Clean
    } else if error == SOFT {
        WriteEnd::Again
    } else if formatted {
        WriteEnd::GaveUp
    } else if format_fatal {
        WriteEnd::FormatFatal
    } else {
        WriteEnd::FormatRetry
    }
}

fn cylinder_count(last: u8) -> u16 {
    u16::from(last).wrapping_add(1)
}

fn format_tracks(
    parm_ok: bool,
    rom_status: u8,
    last: u8,
    sides: u8,
    start_track: u8,
    start_side: u8,
    trouble: Option<(u8, u8)>,
) -> FormatEnd {
    if !parm_ok || rom_status == ROM_REJECTED {
        return FormatEnd::Compatible;
    }
    let mut track = start_track;
    let mut side = start_side;
    let mut count = 0u8;
    if side != sides && track > last {
        return FormatEnd::Finished(0);
    }
    loop {
        if side > sides {
            side = 0;
            track = track.wrapping_add(1);
            if track > last {
                return FormatEnd::Finished(count);
            }
            continue;
        }
        if trouble == Some((track, side)) {
            if side != 1 || track != 0 {
                return FormatEnd::TargetBad;
            }
            return FormatEnd::Compatible;
        }
        count = count.wrapping_add(1);
        side = side.wrapping_add(1);
    }
}

fn format_again(error: u8) -> bool {
    error == SOFT
}

fn cylinders(total: u16, per_track: u16, heads: u16) -> Option<u8> {
    if per_track == 0 || heads == 0 {
        return None;
    }
    Some((total / per_track / heads) as u8)
}

fn source_media(
    media: Geometry,
    target_tracks: u8,
    target_sectors: u8,
    target_heads: u8,
    one_side: bool,
) -> Media {
    let Some(count) = cylinders(media.total, media.per_track, media.heads) else {
        return Media::Old;
    };
    if count > target_tracks {
        return Media::Fatal;
    }
    if media.per_track as u8 > target_sectors {
        return Media::Fatal;
    }
    let sides = if one_side {
        0
    } else if media.heads as u8 > target_heads {
        return Media::Fatal;
    } else {
        media.heads as u8 - 1
    };
    Media::Ready { last: count.wrapping_sub(1), end: media.per_track as u8, sides }
}

fn old_source(probe: Probe, one_side: bool, other_side: bool, target_heads: u8) -> Media {
    let (end, last) = match probe {
        Probe::Failed => return Media::Bad,
        Probe::Eight => (8u8, 39u8),
        Probe::Nine => (9, 39),
        Probe::Fifteen => (15, 79),
    };
    if one_side || !other_side {
        return Media::Ready { last, end, sides: 0 };
    }
    if target_heads == 2 {
        Media::Ready { last, end, sides: 1 }
    } else {
        Media::Fatal
    }
}

fn sector_index(sector: u16) -> u16 {
    sector.wrapping_sub(1)
}

fn sector_failed(error: u8) -> bool {
    error == HARD
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

fn old_record(end: u8, sides: u8) -> (u8, u8) {
    let media = if end > 9 {
        0xF9
    } else if sides != 0 {
        0xFD
    } else {
        0xFC
    };
    (media, end)
}

fn target_media(
    media: Geometry,
    source_last: u8,
    source_end: u8,
    source_bytes: u16,
    source_sides: u8,
) -> Target {
    let Some(count) = cylinders(media.total, media.per_track, media.heads) else {
        return Target::Old;
    };
    let last = count.wrapping_sub(1);
    if last == source_last {
        if source_end != media.per_track as u8 || media.bytes != source_bytes {
            return Target::Format;
        }
        if count_heads(media.heads) < source_sides {
            return Target::Format;
        }
        return Target::Ready;
    }
    if media.per_track != 15 || source_last != FORTY_TRACKS {
        return Target::Format;
    }
    Target::Fatal
}

fn count_heads(heads: u16) -> u8 {
    (heads as u8).wrapping_sub(1)
}

fn old_target(unknown: bool, probe: Probe, source_end: u8) -> Target {
    if unknown {
        return Target::Format;
    }
    match probe {
        Probe::Failed => Target::Format,
        Probe::Eight => {
            if source_end == 15 {
                Target::Fatal
            } else if source_end == 9 {
                Target::Format
            } else {
                Target::Ready
            }
        }
        Probe::Nine => {
            if source_end == 15 { Target::Fatal } else { Target::Ready }
        }
        Probe::Fifteen => {
            if source_end == 15 { Target::Ready } else { Target::Fatal }
        }
    }
}

fn crossed(source_last: u8, target_last: u8) -> bool {
    if source_last == 39 && target_last == 79 {
        true
    } else {
        source_last == 79 && target_last == 39
    }
}

fn needs_media_type(target_tracks: u8, source_last: u8, source_end: u8, target_sectors: u8) -> bool {
    let max_last = target_tracks.wrapping_sub(1);
    source_last < max_last || source_end < target_sectors
}

fn io_fate(scene: &ErrorScene) -> IoFate {
    if scene.code == NOT_READY {
        if scene.formatting {
            return IoFate::Soft { letter: Some(letter(scene.drive)) };
        }
        if scene.trying {
            return IoFate::TimedOut;
        }
        if !scene.on_target || scene.timed_out {
            return IoFate::Soft { letter: Some(letter(scene.drive)) };
        }
        if scene.soft_while_trying {
            return IoFate::Kept;
        }
        return IoFate::Hard { unknown: false };
    }
    if scene.code == PROTECTED {
        return IoFate::Soft { letter: None };
    }
    IoFate::Hard { unknown: scene.code == UNKNOWN_MEDIA }
}

fn letter(logical: u8) -> u8 {
    b'A'.wrapping_add(logical.wrapping_sub(1))
}

fn failure(operation: u8, read_track: u8, write_track: u8) -> (bool, u8) {
    if operation == READ_TRACK {
        (true, read_track)
    } else {
        (false, write_track)
    }
}

fn delivered(response: u8, display_failed: bool) -> (bool, bool) {
    if response == SELECT_PRESENT {
        (true, false)
    } else {
        (false, display_failed)
    }
}

fn answer(selecting: bool, failed: bool, reply: u8) -> Result<u8, ()> {
    if selecting {
        Ok(0)
    } else if failed {
        Err(())
    } else {
        Ok(reply)
    }
}

fn shown_serial(ready: bool, low: u16, high: u16) -> Option<(u16, u16)> {
    if ready { Some((high, low)) } else { None }
}

fn stack_bytes() -> u16 {
    STACK_COPIES * STACK_ENTRY
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DISKCOPY EX");
    put_hex8(FINE_EXIT);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(VERSION_EXIT);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(BREAK_EXIT);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(PARSE_EXIT);
    crate::bios::mscon::con_write_bytes(b" XF");
    put_hex8(EXIT_CALL);
    crate::bios::mscon::con_write_bytes(b" FB");
    put_hex8(FALLBACK_CALL);
    crate::bios::mscon::con_write_bytes(b" BF");
    put_hex16(BUFFER_BEGIN);
    crate::bios::mscon::con_write_bytes(b" BE");
    put_hex16(BUFFER_END);
    crate::bios::mscon::con_write_bytes(b" LT");
    put_hex8(LAST_TRACK);
    crate::bios::mscon::con_write_bytes(b" ET");
    put_hex8(END_OF_TRACK);
    crate::bios::mscon::con_write_bytes(b" SZ");
    put_hex16(SECTOR_BYTES);
    crate::bios::mscon::con_write_bytes(b" SB");
    put_hex16(stack_bytes());
    crate::bios::mscon::con_write_bytes(b" PB");
    put_hex16(BPB_BYTES);
    crate::bios::mscon::con_write_bytes(b" CT");
    put_hex8(SINGLE_DRIVE);
    write_exit(false, true, false);
    write_exit(true, false, false);
    write_exit(true, true, true);
    write_exit(true, true, false);
    crate::bios::mscon::con_write_bytes(if owns(0) { b" CL440F" } else { b" SK" });
    crate::bios::mscon::con_write_bytes(if owns(1) { b" CL440F" } else { b" SK" });
    write_flag(b" S", parm_restore(1));
    write_flag(b" T", parm_restore(0));
    crate::bios::mscon::con_write_bytes(if prompts_once(SINGLE_DRIVE) { b" T2" } else { b" T1" });
    crate::bios::mscon::con_write_bytes(if prompts_once(DOUBLE_DRIVE) { b" T2" } else { b" T1" });
    write_copy(copy_end(true, false, 0));
    write_copy(copy_end(false, true, 5));
    write_copy(copy_end(false, false, 0));
    write_copy(copy_end(false, false, 3));
    write_go(0, 79);
    write_go(80, 79);
    write_go(79, 79);
    write_again(false, 1);
    write_again(false, 0);
    write_again(false, 2);
    write_again(true, 1);
    write_gate(may_fill(0, true));
    write_gate(may_fill(0, false));
    write_gate(may_fill(2, true));
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(filled(5, 0x0240, 0x1000, 0x1480));
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(filled(5, 0x0240, 0x1000, 0x147F));
    write_gate(may_fill(0, true));
    write_gate(may_fill(0, false));
    write_gate(may_fill(2, true));
    crate::bios::mscon::con_write_bytes(b" WN");
    put_hex8(written(5, 0x0240, 0x1000, 0x1480, None));
    crate::bios::mscon::con_write_bytes(b" WN");
    put_hex8(written(5, 0x0240, 0x1000, 0x1480, Some(0)));
    crate::bios::mscon::con_write_bytes(b" WN");
    put_hex8(written(5, 0x0240, 0x1000, 0x1480, Some(1)));
    crate::bios::mscon::con_write_bytes(b" PS");
    put_hex8(passes(0, false));
    crate::bios::mscon::con_write_bytes(b" PS");
    put_hex8(passes(1, false));
    crate::bios::mscon::con_write_bytes(b" PS");
    put_hex8(passes(1, true));
    crate::bios::mscon::con_write_bytes(b" FS");
    put_hex16(0);
    write_read(read_end(NO_ERROR, 0, 0, true));
    write_read(read_end(NO_ERROR, 1, 0, true));
    write_read(read_end(SOFT, 0, 0, true));
    write_read(read_end(HARD, 4, 1, true));
    write_read(read_end(HARD, 4, 1, false));
    write_hash(hashed(0xF9, 0x29, 0x07C0, 0x0102, 0x0A0B, 0x0304));
    write_hash(hashed(0xF9, 0x00, 0x07C0, 0x0102, 0x0A0B, 0x0304));
    write_hash(hashed(0x01, 0x29, 0x07C0, 0x0102, 0x0A0B, 0x0304));
    write_hash(hashed(0xF0, 0x28, 0x07C0, 0x0102, 0x0A0B, 0x0304));
    write_write(write_end(NO_ERROR, false, false));
    write_write(write_end(SOFT, false, false));
    write_write(write_end(HARD, true, false));
    write_write(write_end(HARD, false, true));
    write_write(write_end(HARD, false, false));
    crate::bios::mscon::con_write_bytes(b" NC");
    put_hex16(cylinder_count(LAST_TRACK));
    crate::bios::mscon::con_write_bytes(b" BF");
    put_hex8(SET_BEFORE_FORMAT);
    crate::bios::mscon::con_write_bytes(b" RF");
    put_hex8(FORMAT_SPECIAL);
    write_format(format_tracks(false, 0, 1, 1, 0, 0, None));
    write_format(format_tracks(true, ROM_REJECTED, 1, 1, 0, 0, None));
    write_format(format_tracks(true, 0, 1, 1, 0, 0, None));
    write_format(format_tracks(true, 0, 5, 0, 5, 0, Some((5, 0))));
    write_format(format_tracks(true, 0, 0, 1, 0, 1, Some((0, 1))));
    write_format(format_tracks(true, 0, 3, 1, 3, 1, Some((3, 1))));
    write_format(format_tracks(true, 0, 0, 0, 5, 0, None));
    write_format(format_tracks(true, 0, 0, 1, 5, 0, None));
    crate::bios::mscon::con_write_bytes(if format_again(SOFT) { b" RT" } else { b" HD" });
    crate::bios::mscon::con_write_bytes(if format_again(HARD) { b" RT" } else { b" HD" });
    crate::bios::mscon::con_write_bytes(if format_again(NO_ERROR) { b" RT" } else { b" OK" });
    write_media(source_media(Geometry { total: 40, per_track: 10, heads: 1, bytes: 512 }, 4, 10, 1, false));
    write_media(source_media(Geometry { total: 40, per_track: 10, heads: 1, bytes: 512 }, 3, 10, 1, false));
    write_media(source_media(Geometry { total: 18, per_track: 9, heads: 1, bytes: 512 }, 2, 9, 1, false));
    write_media(source_media(Geometry { total: 20, per_track: 10, heads: 2, bytes: 512 }, 2, 10, 1, true));
    write_media(source_media(Geometry { total: 20, per_track: 10, heads: 2, bytes: 512 }, 2, 10, 1, false));
    write_media(source_media(Geometry { total: 20, per_track: 0, heads: 1, bytes: 512 }, 2, 10, 1, false));
    write_media(old_source(Probe::Failed, false, true, 2));
    write_media(old_source(Probe::Eight, false, false, 2));
    write_media(old_source(Probe::Nine, true, true, 2));
    write_media(old_source(Probe::Fifteen, false, true, 2));
    write_media(old_source(Probe::Fifteen, false, true, 1));
    write_media(old_source(Probe::Eight, false, true, 2));
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex16(sector_index(1));
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex16(sector_index(9));
    crate::bios::mscon::con_write_bytes(if sector_failed(SOFT) { b" CY" } else { b" RT" });
    crate::bios::mscon::con_write_bytes(if sector_failed(HARD) { b" CY" } else { b" NC" });
    crate::bios::mscon::con_write_bytes(if sector_failed(NO_ERROR) { b" CY" } else { b" NC" });
    let (bytes, paragraphs, secondary, primary) = track_layout(512, 9, 1, BUFFER_BEGIN);
    crate::bios::mscon::con_write_bytes(b" TB");
    put_hex16(bytes);
    crate::bios::mscon::con_write_bytes(b" PQ");
    put_hex8(paragraphs);
    crate::bios::mscon::con_write_bytes(b" TS");
    put_hex16(primary.wrapping_sub(secondary));
    crate::bios::mscon::con_write_bytes(b" BG");
    put_hex16(primary);
    crate::bios::mscon::con_write_bytes(if fits(primary, BUFFER_END, primary.wrapping_sub(secondary)) {
        b" OK"
    } else {
        b" FT"
    });
    crate::bios::mscon::con_write_bytes(if fits(0x1240, 0x1250, 0x0240) { b" OK" } else { b" FT" });
    write_pair(old_record(15, 0));
    write_pair(old_record(9, 1));
    write_pair(old_record(9, 0));
    write_pair(old_record(8, 0));
    crate::bios::mscon::con_write_bytes(b" LN");
    put_hex16(BPB_BYTES);
    crate::bios::mscon::con_write_bytes(b" N");
    put_hex16(u16::from(END_OF_TRACK));
    crate::bios::mscon::con_write_bytes(b" F");
    put_hex16(1);
    put_hex16(SECTOR_BYTES);
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex16(u16::from(END_OF_TRACK));
    put_hex16(SECTOR_BYTES);
    write_target(target_media(Geometry { total: 40, per_track: 10, heads: 1, bytes: 512 }, 3, 10, 512, 0));
    write_target(target_media(Geometry { total: 18, per_track: 9, heads: 1, bytes: 512 }, 3, 15, 512, 0));
    write_target(target_media(Geometry { total: 2400, per_track: 15, heads: 2, bytes: 512 }, FORTY_TRACKS, 15, 512, 1));
    write_target(target_media(Geometry { total: 40, per_track: 10, heads: 1, bytes: 256 }, 3, 10, 512, 0));
    write_target(target_media(Geometry { total: 40, per_track: 10, heads: 1, bytes: 512 }, 3, 10, 512, 1));
    write_target(target_media(Geometry { total: 20, per_track: 0, heads: 1, bytes: 512 }, 3, 10, 512, 0));
    write_target(old_target(true, Probe::Fifteen, 15));
    write_target(old_target(false, Probe::Failed, 8));
    write_target(old_target(false, Probe::Eight, 15));
    write_target(old_target(false, Probe::Eight, 9));
    write_target(old_target(false, Probe::Eight, 8));
    write_target(old_target(false, Probe::Nine, 15));
    write_target(old_target(false, Probe::Nine, 9));
    write_target(old_target(false, Probe::Fifteen, 15));
    write_target(old_target(false, Probe::Fifteen, 9));
    write_cross(crossed(39, 79));
    write_cross(crossed(79, 39));
    write_cross(crossed(39, 39));
    write_cross(crossed(79, 79));
    write_cross(crossed(40, 79));
    crate::bios::mscon::con_write_bytes(if needs_media_type(80, 39, 15, 15) { b" M01" } else { b" M00" });
    crate::bios::mscon::con_write_bytes(if needs_media_type(40, 39, 9, 15) { b" M01" } else { b" M00" });
    crate::bios::mscon::con_write_bytes(if needs_media_type(40, 39, 15, 15) { b" M01" } else { b" M00" });
    crate::bios::mscon::con_write_bytes(b" GN");
    put_hex16(IOCTL_CALL);
    put_hex8(MAJOR);
    write_io(&ErrorScene { code: NOT_READY, formatting: true, trying: false, on_target: false, timed_out: false, soft_while_trying: false, drive: 1 });
    write_io(&ErrorScene { code: NOT_READY, formatting: false, trying: true, on_target: true, timed_out: false, soft_while_trying: false, drive: 1 });
    write_io(&ErrorScene { code: NOT_READY, formatting: false, trying: true, on_target: true, timed_out: true, soft_while_trying: false, drive: 1 });
    write_io(&ErrorScene { code: NOT_READY, formatting: false, trying: false, on_target: false, timed_out: false, soft_while_trying: false, drive: 3 });
    write_io(&ErrorScene { code: NOT_READY, formatting: false, trying: false, on_target: true, timed_out: true, soft_while_trying: false, drive: 1 });
    write_io(&ErrorScene { code: NOT_READY, formatting: false, trying: false, on_target: true, timed_out: false, soft_while_trying: true, drive: 1 });
    write_io(&ErrorScene { code: NOT_READY, formatting: false, trying: false, on_target: true, timed_out: false, soft_while_trying: false, drive: 1 });
    write_io(&ErrorScene { code: PROTECTED, formatting: false, trying: false, on_target: true, timed_out: false, soft_while_trying: false, drive: 1 });
    write_io(&ErrorScene { code: UNKNOWN_MEDIA, formatting: false, trying: false, on_target: true, timed_out: false, soft_while_trying: false, drive: 1 });
    write_io(&ErrorScene { code: 5, formatting: false, trying: false, on_target: true, timed_out: false, soft_while_trying: false, drive: 1 });
    let (reading, track) = failure(READ_TRACK, 4, 7);
    write_where(reading, 1, track, 1);
    let (reading, track) = failure(WRITE_TRACK, 4, 7);
    write_where(reading, 0, track, 2);
    crate::bios::mscon::con_write_bytes(b" MX");
    put_hex16(SELECT_CALL);
    write_delivery(delivered(SELECT_PRESENT, true));
    write_delivery(delivered(0, false));
    write_delivery(delivered(0, true));
    crate::bios::mscon::con_write_bytes(b" YN");
    put_hex16(YESNO_CALL);
    write_answer(answer(true, false, 1));
    write_answer(answer(false, true, 1));
    write_answer(answer(false, false, 1));
    crate::bios::mscon::con_write_bytes(b" AT");
    put_hex16(SERIAL_LOW_AT);
    crate::bios::mscon::con_write_bytes(b" AT");
    put_hex16(SERIAL_HIGH_AT);
    write_volume(shown_serial(false, 0x0406, 0x11CB));
    write_volume(shown_serial(true, 0x0406, 0x11CB));
    crate::bios::mscon::con_write_bytes(b" PA\r\n");
}

fn write_exit(loaded: bool, ready: bool, broken: bool) {
    let (code, restore) = exit_code(loaded, ready, broken);
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex8(code);
    crate::bios::mscon::con_write_bytes(if restore { b" RS" } else { b" NR" });
}

fn write_flag(tag: &[u8], function: Option<u8>) {
    crate::bios::mscon::con_write_bytes(tag);
    match function {
        Some(function) => put_hex8(function),
        None => crate::bios::mscon::con_write_bytes(b"SK"),
    }
}

fn write_copy(end: CopyEnd) {
    crate::bios::mscon::con_write_bytes(b" UN");
    put_hex8(u8::from(end.unusable));
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex8(u8::from(end.fatal));
    crate::bios::mscon::con_write_bytes(b" VL");
    put_hex8(u8::from(end.volser));
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

fn write_gate(open: bool) {
    crate::bios::mscon::con_write_bytes(if open { b" FL" } else { b" NF" });
}

fn write_read(end: ReadEnd) {
    let tag: &[u8] = match end {
        ReadEnd::Adjusted => b" AD",
        ReadEnd::Clean => b" OK",
        ReadEnd::Again => b" RT",
        ReadEnd::Reported => b" HR",
        ReadEnd::Silent => b" HS",
    };
    crate::bios::mscon::con_write_bytes(tag);
}

fn write_hash(serial: Option<(u16, u16)>) {
    match serial {
        Some((low, high)) => {
            crate::bios::mscon::con_write_bytes(b" LW");
            put_hex16(low);
            crate::bios::mscon::con_write_bytes(b" HW");
            put_hex16(high);
        }
        None => crate::bios::mscon::con_write_bytes(b" KP"),
    }
}

fn write_write(end: WriteEnd) {
    let tag: &[u8] = match end {
        WriteEnd::Clean => b" OK",
        WriteEnd::Again => b" RT",
        WriteEnd::GaveUp => b" GU",
        WriteEnd::FormatFatal => b" FF",
        WriteEnd::FormatRetry => b" RW",
    };
    crate::bios::mscon::con_write_bytes(tag);
}

fn write_format(end: FormatEnd) {
    match end {
        FormatEnd::Compatible => crate::bios::mscon::con_write_bytes(b" FC"),
        FormatEnd::TargetBad => crate::bios::mscon::con_write_bytes(b" TB"),
        FormatEnd::Finished(count) => {
            crate::bios::mscon::con_write_bytes(b" DN");
            put_hex8(count);
        }
    }
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

fn write_pair(record: (u8, u8)) {
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(record.0);
    put_hex8(record.1);
}

fn write_target(target: Target) {
    let tag: &[u8] = match target {
        Target::Ready => b" OK",
        Target::Format => b" FM",
        Target::Fatal => b" FT",
        Target::Old => b" OL",
    };
    crate::bios::mscon::con_write_bytes(tag);
}

fn write_cross(crossed: bool) {
    crate::bios::mscon::con_write_bytes(if crossed { b" CY" } else { b" NC" });
}

fn write_io(scene: &ErrorScene) {
    match io_fate(scene) {
        IoFate::Soft { letter } => {
            crate::bios::mscon::con_write_bytes(b" IO01");
            match letter {
                Some(letter) => {
                    crate::bios::mscon::con_write_bytes(b" L");
                    put_hex8(letter);
                }
                None => crate::bios::mscon::con_write_bytes(b" NL"),
            }
        }
        IoFate::Hard { unknown } => {
            crate::bios::mscon::con_write_bytes(if unknown { b" IO02 UK01" } else { b" IO02 UK00" });
        }
        IoFate::TimedOut => crate::bios::mscon::con_write_bytes(b" TO"),
        IoFate::Kept => crate::bios::mscon::con_write_bytes(b" KP"),
    }
}

fn write_where(reading: bool, side: u8, track: u8, drive: u8) {
    crate::bios::mscon::con_write_bytes(if reading { b" RD S" } else { b" WR S" });
    put_hex8(side);
    crate::bios::mscon::con_write_bytes(b" T");
    put_hex8(track);
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex8(letter(drive));
}

fn write_delivery(result: (bool, bool)) {
    crate::bios::mscon::con_write_bytes(if result.0 { b" SP" } else { b" SA" });
    crate::bios::mscon::con_write_bytes(if result.1 { b" ST" } else { b" CL" });
}

fn write_answer(result: Result<u8, ()>) {
    match result {
        Ok(0) => crate::bios::mscon::con_write_bytes(b" SN00"),
        Ok(value) => {
            crate::bios::mscon::con_write_bytes(b" AN");
            put_hex8(value);
        }
        Err(()) => crate::bios::mscon::con_write_bytes(b" CY"),
    }
}

fn write_volume(serial: Option<(u16, u16)>) {
    match serial {
        None => crate::bios::mscon::con_write_bytes(b" SK"),
        Some((high, low)) => {
            crate::bios::mscon::con_write_bytes(b" VH");
            put_hex16(high);
            crate::bios::mscon::con_write_bytes(b" VL");
            put_hex16(low);
        }
    }
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
