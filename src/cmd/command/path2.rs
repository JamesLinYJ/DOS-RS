//! Command lookup from `CMD/COMMAND/PATH2.ASM`.
//!
//! `.COM` outranks `.EXE`, which outranks `.BAT`. A requested extension stops
//! at the first real match and returns that type. A name with no extension is
//! copied with `.???` in its place. The saved argument block is rounded up to
//! paragraphs.

const COM: u8 = 8;
const EXE: u8 = 4;
const BAT: u8 = 2;
const NONE: u8 = 0;
const NAME_LIMIT: usize = 13;
const ARG_BLOCK: u16 = 1348;

fn drive_number(name: &[u8]) -> u8 {
    if name.len() >= 2 && name[1] == b':' {
        (name[0] & !0x20).wrapping_sub(b'@')
    } else {
        0
    }
}

fn file_type(name: &[u8]) -> u8 {
    let mut end = 0usize;
    while end < name.len() && end < NAME_LIMIT {
        if name[end] == 0 {
            if end >= 4 {
                let tail = &name[end - 4..end];
                if tail == b".COM" {
                    return COM;
                }
                if tail == b".EXE" {
                    return EXE;
                }
                if tail == b".BAT" {
                    return BAT;
                }
            }
            return NONE;
        }
        end += 1;
    }
    NONE
}

fn best(names: &[&[u8]], extension_given: bool, directory_ok: bool) -> u8 {
    if !directory_ok {
        return NONE;
    }
    let mut winner = NONE;
    let mut recorded = if extension_given { 0 } else { 1 };
    let mut index = 0usize;
    while index < names.len() {
        let mut kind = file_type(names[index]);
        index += 1;
        if recorded != 1 && kind != NONE {
            recorded = kind;
            kind = COM;
        }
        if kind > winner {
            winner = kind;
            if kind == COM {
                break;
            }
        }
    }
    if recorded == 1 { winner } else { recorded }
}

fn strip(name: &[u8], last: usize, capacity: usize) -> Option<[u8; 16]> {
    if last >= name.len() || name[last] == 0 {
        return None;
    }
    let mut end = 0usize;
    while end < name.len() && name[end] != 0 {
        end += 1;
    }
    let mut saw_dot = false;
    let mut index = last;
    while index <= end {
        if name[index] == b'.' {
            saw_dot = true;
            break;
        }
        index += 1;
    }
    let copy_len = end + 1;
    if copy_len > capacity.wrapping_sub(4) {
        return None;
    }
    let mut out = [0u8; 16];
    let mut used = 0usize;
    while used < end && used < out.len() {
        out[used] = name[used];
        used += 1;
    }
    if !saw_dot && used + 4 < out.len() {
        out[used] = b'.';
        out[used + 1] = b'?';
        out[used + 2] = b'?';
        out[used + 3] = b'?';
    }
    Some(out)
}

fn save_paragraphs(extra: u16) -> u16 {
    (extra.wrapping_add(ARG_BLOCK).wrapping_add(15)) >> 4
}

fn adjusted(base: u16, pointer: u16) -> u16 {
    pointer.wrapping_sub(base)
}

fn write_name(label: &[u8], name: &[u8]) {
    crate::bios::mscon::con_write_bytes(label);
    let mut index = 0usize;
    while index < name.len() && name[index] != 0 {
        put_hex8(name[index]);
        index += 1;
    }
}

pub fn show() {
    let com = b"FOO.COM\0";
    let exe = b"FOO.EXE\0";
    let bat = b"FOO.BAT\0";
    let text = b"FOO.TXT\0";
    crate::bios::mscon::con_write_bytes(b"PATH2 DR");
    put_hex8(drive_number(b"a:FOO"));
    crate::bios::mscon::con_write_bytes(b" DR");
    put_hex8(drive_number(b"C:X"));
    crate::bios::mscon::con_write_bytes(b" DR");
    put_hex8(drive_number(b"FOO"));
    crate::bios::mscon::con_write_bytes(b" BD");
    put_hex8(best(&[com], false, false));
    crate::bios::mscon::con_write_bytes(b" NC");
    put_hex8(best(&[com], false, true));
    crate::bios::mscon::con_write_bytes(b" NE");
    put_hex8(best(&[bat, exe], false, true));
    crate::bios::mscon::con_write_bytes(b" NB");
    put_hex8(best(&[exe, bat], false, true));
    crate::bios::mscon::con_write_bytes(b" SB");
    put_hex8(best(&[bat, com], true, true));
    crate::bios::mscon::con_write_bytes(b" SX");
    put_hex8(best(&[text], true, true));
    if let Some(wild) = strip(b"FOO\0", 0, 8) {
        write_name(b" W", &wild);
    }
    if let Some(kept) = strip(b"FOO.COM\0", 0, 16) {
        write_name(b" K", &kept);
    }
    crate::bios::mscon::con_write_bytes(b" E");
    put_hex8(u8::from(strip(b"\0", 0, 16).is_some()));
    crate::bios::mscon::con_write_bytes(b" E");
    put_hex8(u8::from(strip(b"FOO\0", 0, 7).is_some()));
    crate::bios::mscon::con_write_bytes(b" SV");
    put_hex16(save_paragraphs(1));
    crate::bios::mscon::con_write_bytes(b" SV");
    put_hex16(save_paragraphs(16));
    crate::bios::mscon::con_write_bytes(b" SV");
    put_hex16(0);
    crate::bios::mscon::con_write_bytes(b" AD");
    put_hex16(adjusted(0x1000, 0x12C4));
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
