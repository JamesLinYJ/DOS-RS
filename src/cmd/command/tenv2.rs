//! Directory commands from `CMD/COMMAND/TENV2.ASM`.
//!
//! `CHDIR` with no path prints the current directory. A wildcard path is
//! rejected before the call. `MKDIR` reports an existing directory only after
//! access denied and a directory match. `RMDIR` uses the old message for both
//! a missing path and access denied. A slash is a path character only when it
//! is not the switch character.

const PATH_NOT_FOUND: u16 = 3;
const ACCESS_DENIED: u16 = 5;
const SHOW: u8 = 0;
const SETUP_ERROR: u8 = 1;
const OLD_MESSAGE: u8 = 2;
const EXTENDED: u8 = 3;
const ALREADY_EXISTS: u8 = 4;
const WILD: u8 = 6;
const CHANGED: u8 = 7;

fn path_char(switch_char: u8, byte: u8) -> bool {
    if switch_char != b'/' && byte == b'/' {
        return true;
    }
    byte == b'\\'
}

fn change_dir(
    ended: bool,
    parsed: bool,
    drive: bool,
    extra: bool,
    wild: bool,
    failure: Option<u16>,
) -> u8 {
    if ended {
        return SHOW;
    }
    if !parsed {
        return SETUP_ERROR;
    }
    if drive {
        return if extra { SETUP_ERROR } else { SHOW };
    }
    if extra {
        return SETUP_ERROR;
    }
    if wild {
        return WILD;
    }
    match failure {
        None => CHANGED,
        Some(PATH_NOT_FOUND) => OLD_MESSAGE,
        Some(_) => EXTENDED,
    }
}

fn make_dir(ready: bool, failure: Option<u16>, found: bool, directory: bool) -> u8 {
    if !ready {
        return SETUP_ERROR;
    }
    match failure {
        None => SHOW,
        Some(PATH_NOT_FOUND) => OLD_MESSAGE,
        Some(ACCESS_DENIED) => {
            if found && directory {
                ALREADY_EXISTS
            } else {
                OLD_MESSAGE
            }
        }
        Some(_) => EXTENDED,
    }
}

fn remove_dir(failed_setup: bool, more_arguments: bool, failure: Option<u16>) -> u8 {
    if failed_setup {
        return SETUP_ERROR;
    }
    if more_arguments {
        return OLD_MESSAGE;
    }
    match failure {
        None => SHOW,
        Some(PATH_NOT_FOUND) | Some(ACCESS_DENIED) => OLD_MESSAGE,
        Some(_) => EXTENDED,
    }
}

fn fcb_text(name: &[u8; 8], extension: &[u8; 3]) -> [u8; 13] {
    let mut out = [0u8; 13];
    let mut used = 0usize;
    let mut index = 0usize;
    while index < 8 {
        if name[index] != b' ' {
            out[used] = name[index];
            used += 1;
        }
        index += 1;
    }
    if extension[0] != b' ' {
        out[used] = b'.';
        used += 1;
        out[used] = extension[0];
        used += 1;
        index = 1;
        while index < 3 {
            if extension[index] == b' ' {
                break;
            }
            out[used] = extension[index];
            used += 1;
            index += 1;
        }
    }
    out
}

fn same_text(left: &[u8], right: &[u8]) -> bool {
    let mut index = 0usize;
    loop {
        let left_byte = if index < left.len() { left[index] } else { 0 };
        let right_byte = if index < right.len() { right[index] } else { 0 };
        if left_byte != right_byte {
            return false;
        }
        if left_byte == 0 {
            return true;
        }
        index += 1;
    }
}

fn write_text(label: &[u8], text: &[u8]) {
    crate::bios::mscon::con_write_bytes(label);
    let mut index = 0usize;
    while index < text.len() && text[index] != 0 {
        put_hex8(text[index]);
        index += 1;
    }
}

pub fn show() {
    let dir = fcb_text(b"DIR     ", b"   ");
    let command = fcb_text(b"COMMAND ", b"COM");
    crate::bios::mscon::con_write_bytes(b"TENV2 CD");
    put_hex8(change_dir(true, true, false, false, false, None));
    crate::bios::mscon::con_write_bytes(b" CD");
    put_hex8(change_dir(false, false, false, false, false, None));
    crate::bios::mscon::con_write_bytes(b" CD");
    put_hex8(change_dir(false, true, true, false, false, None));
    crate::bios::mscon::con_write_bytes(b" CD");
    put_hex8(change_dir(false, true, true, true, false, None));
    crate::bios::mscon::con_write_bytes(b" CD");
    put_hex8(change_dir(false, true, false, false, true, None));
    crate::bios::mscon::con_write_bytes(b" CD");
    put_hex8(change_dir(false, true, false, false, false, Some(3)));
    crate::bios::mscon::con_write_bytes(b" CD");
    put_hex8(change_dir(false, true, false, false, false, Some(5)));
    crate::bios::mscon::con_write_bytes(b" CD");
    put_hex8(change_dir(false, true, false, false, false, None));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(make_dir(false, None, false, false));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(make_dir(true, None, false, false));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(make_dir(true, Some(3), false, false));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(make_dir(true, Some(5), true, true));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(make_dir(true, Some(5), false, false));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(make_dir(true, Some(5), true, false));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(make_dir(true, Some(2), false, false));
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex8(remove_dir(true, false, None));
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex8(remove_dir(false, true, None));
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex8(remove_dir(false, false, None));
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex8(remove_dir(false, false, Some(3)));
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex8(remove_dir(false, false, Some(5)));
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex8(remove_dir(false, false, Some(2)));
    crate::bios::mscon::con_write_bytes(b" PC");
    put_hex8(u8::from(path_char(b'/', b'/')));
    crate::bios::mscon::con_write_bytes(b" PC");
    put_hex8(u8::from(path_char(b'/', b'\\')));
    crate::bios::mscon::con_write_bytes(b" PC");
    put_hex8(u8::from(path_char(b'-', b'/')));
    crate::bios::mscon::con_write_bytes(b" PC");
    put_hex8(u8::from(path_char(b'-', b'A')));
    write_text(b" FC", &dir);
    write_text(b" FC", &command);
    crate::bios::mscon::con_write_bytes(b" EQ");
    put_hex8(u8::from(same_text(b"DIR\0", b"DIR\0")));
    crate::bios::mscon::con_write_bytes(b" EQ");
    put_hex8(u8::from(same_text(b"DIR\0", b"BIN\0")));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
