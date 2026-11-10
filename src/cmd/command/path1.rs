//! Program search from `CMD/COMMAND/PATH1.ASM`.
//!
//! A wildcard or a switch is not a command name. The separator is `/` when
//! that character is a path character, and `\` otherwise. A failed search of
//! a complete path stops. A relative name walks `PATH`, skipping an empty
//! piece and not doubling a separator.

const WILD: u8 = 0x02;
const SWITCH: u8 = 0x01;

fn rejected(flags: u8) -> bool {
    flags & (WILD + SWITCH) != 0
}

fn choose_separator(slash_is_path: bool) -> u8 {
    if slash_is_path { b'/' } else { b'\\' }
}

fn has_drive(name: &[u8]) -> bool {
    name.len() >= 2 && name[1] == b':'
}

fn keep_walking(found: bool, absolute: bool) -> bool {
    if found {
        return false;
    }
    !absolute
}

struct Piece {
    next: usize,
    copied: bool,
    finished: bool,
}

fn piece(path: &[u8], at: usize, extra: u8) -> Piece {
    let mut cursor = at;
    let mut copied = false;
    let mut finished = true;
    while cursor < path.len() {
        let byte = path[cursor];
        cursor += 1;
        if byte == 0 {
            break;
        }
        if byte == extra {
            finished = false;
            break;
        }
        copied = true;
    }
    Piece { next: cursor, copied, finished }
}

fn join(segment: &[u8], file: &[u8], separator: u8) -> [u8; 8] {
    let mut out = [0u8; 8];
    let mut used = 0usize;
    let mut index = 0usize;
    while index < segment.len() && segment[index] != 0 && used < out.len() {
        out[used] = segment[index];
        used += 1;
        index += 1;
    }
    if (used == 0 || out[used - 1] != separator) && used < out.len() {
        out[used] = separator;
        used += 1;
    }
    index = 0;
    while index < file.len() && file[index] != 0 && used < out.len() {
        out[used] = file[index];
        used += 1;
        index += 1;
    }
    out
}

pub fn show() {
    let joined = join(b"BIN", b"FOO", b'\\');
    let plain = join(b"BIN\\", b"FOO", b'\\');
    let empty = piece(b";BIN\0", 0, b';');
    let done = piece(b"BIN\0", 0, b';');
    crate::bios::mscon::con_write_bytes(b"PATH1 FL");
    put_hex8(u8::from(rejected(WILD)));
    crate::bios::mscon::con_write_bytes(b" FL");
    put_hex8(u8::from(rejected(SWITCH)));
    crate::bios::mscon::con_write_bytes(b" FL");
    put_hex8(u8::from(rejected(0)));
    crate::bios::mscon::con_write_bytes(b" PS");
    put_hex8(choose_separator(true));
    crate::bios::mscon::con_write_bytes(b" PS");
    put_hex8(choose_separator(false));
    crate::bios::mscon::con_write_bytes(b" DR");
    put_hex8(u8::from(has_drive(b"A:FOO")));
    crate::bios::mscon::con_write_bytes(b" DR");
    put_hex8(u8::from(has_drive(b"FOO")));
    crate::bios::mscon::con_write_bytes(b" KS");
    put_hex8(u8::from(keep_walking(true, false)));
    crate::bios::mscon::con_write_bytes(b" KS");
    put_hex8(u8::from(keep_walking(false, true)));
    crate::bios::mscon::con_write_bytes(b" KS");
    put_hex8(u8::from(keep_walking(false, false)));
    crate::bios::mscon::con_write_bytes(b" EM");
    put_hex8(u8::from(empty.copied));
    put_hex8(u8::from(empty.finished));
    put_hex8(empty.next as u8);
    crate::bios::mscon::con_write_bytes(b" DN");
    put_hex8(u8::from(done.copied));
    put_hex8(u8::from(done.finished));
    put_hex8(done.next as u8);
    crate::bios::mscon::con_write_bytes(b" JN");
    put_hex8(joined[0]);
    put_hex8(joined[1]);
    put_hex8(joined[2]);
    put_hex8(joined[3]);
    put_hex8(joined[4]);
    put_hex8(joined[5]);
    put_hex8(joined[6]);
    crate::bios::mscon::con_write_bytes(b" ND");
    put_hex8(plain[0]);
    put_hex8(plain[1]);
    put_hex8(plain[2]);
    put_hex8(plain[3]);
    put_hex8(plain[4]);
    put_hex8(plain[5]);
    put_hex8(plain[6]);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
