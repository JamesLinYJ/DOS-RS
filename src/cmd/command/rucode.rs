//! Resident critical-error handling from `CMD/COMMAND/RUCODE.ASM`.
//!
//! A hard error on a block device is a bad FAT. Otherwise the action is read
//! when bit 0 is clear and write when it is set. Codes below write-protect
//! become general failure. Only FCB-unavailable and sharing-buffer are the
//! new one-line style. Abort is always offered. Ignore is tested before
//! retry, and `/F` fails without asking. `N` keeps a batch file.

const WRITE_PROTECT: u16 = 19;
const GENERAL: u16 = 31;
const FCB_UNAVAILABLE: u16 = 35;
const SHARING_BUFFER: u16 = 36;
const DISK_FULL: u16 = 39;
const RETRY_BIT: u8 = 0x10;
const IGNORE_BIT: u8 = 0x20;
const FAIL_BIT: u8 = 0x08;
const HARD_BIT: u8 = 0x80;
const CHARACTER_BIT: u8 = 0x80;

fn bad_fat(error_flags: u8, device_high: u8) -> bool {
    if error_flags & HARD_BIT == 0 {
        return false;
    }
    device_high & CHARACTER_BIT == 0
}

fn writing(error_flags: u8) -> bool {
    error_flags & 1 != 0
}

fn error_index(code: u16) -> u16 {
    if code < WRITE_PROTECT {
        GENERAL - WRITE_PROTECT
    } else {
        code - WRITE_PROTECT
    }
}

fn new_style(index: u16) -> bool {
    if index == FCB_UNAVAILABLE - WRITE_PROTECT {
        return true;
    }
    index == SHARING_BUFFER - WRITE_PROTECT
}

fn known_error(index: u16) -> bool {
    index <= DISK_FULL - WRITE_PROTECT
}

fn prompt_mask(info: u8) -> u8 {
    let mut mask = 1u8;
    if info & RETRY_BIT != 0 {
        mask |= 2;
    }
    if info & IGNORE_BIT != 0 {
        mask |= 4;
    }
    if info & FAIL_BIT != 0 {
        mask |= 8;
    }
    mask
}

fn reply(letter: u8, info: u8, force_fail: bool) -> Option<u8> {
    if force_fail {
        return Some(3);
    }
    let letter = if letter.is_ascii_lowercase() { letter - 0x20 } else { letter };
    if info & IGNORE_BIT != 0 && letter == b'I' {
        return Some(0);
    }
    if info & RETRY_BIT != 0 && letter == b'R' {
        return Some(1);
    }
    if letter == b'A' {
        return Some(2);
    }
    if info & FAIL_BIT != 0 && letter == b'F' {
        return Some(3);
    }
    None
}

fn end_batch(letter: u8) -> Option<bool> {
    let letter = if letter.is_ascii_lowercase() { letter - 0x20 } else { letter };
    if letter == b'N' {
        return Some(false);
    }
    if letter == b'Y' {
        return Some(true);
    }
    None
}

fn abort_for(index: u16) -> bool {
    index == 0 || index == 2
}

fn asks(character_device: bool, loading: bool) -> bool {
    if character_device {
        return true;
    }
    !loading
}

fn batch_code(answer: Option<bool>) -> u8 {
    match answer {
        Some(true) => 1,
        Some(false) => 0,
        None => 0xFF,
    }
}

fn write_opt(label: &[u8], value: Option<u8>) {
    crate::bios::mscon::con_write_bytes(label);
    put_hex8(value.unwrap_or(0xFF));
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"RUCODE FT");
    put_hex8(u8::from(bad_fat(0x80, 0)));
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex8(u8::from(bad_fat(0x80, 0x80)));
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex8(u8::from(bad_fat(0, 0)));
    crate::bios::mscon::con_write_bytes(b" RW");
    put_hex8(u8::from(writing(0)));
    crate::bios::mscon::con_write_bytes(b" RW");
    put_hex8(u8::from(writing(1)));
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex8(error_index(10) as u8);
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex8(error_index(19) as u8);
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex8(error_index(35) as u8);
    crate::bios::mscon::con_write_bytes(b" NS");
    put_hex8(u8::from(new_style(16)));
    crate::bios::mscon::con_write_bytes(b" NS");
    put_hex8(u8::from(new_style(15)));
    crate::bios::mscon::con_write_bytes(b" NS");
    put_hex8(u8::from(new_style(17)));
    crate::bios::mscon::con_write_bytes(b" KN");
    put_hex8(u8::from(known_error(20)));
    crate::bios::mscon::con_write_bytes(b" KN");
    put_hex8(u8::from(known_error(21)));
    crate::bios::mscon::con_write_bytes(b" PR");
    put_hex8(prompt_mask(0));
    crate::bios::mscon::con_write_bytes(b" PR");
    put_hex8(prompt_mask(RETRY_BIT));
    crate::bios::mscon::con_write_bytes(b" PR");
    put_hex8(prompt_mask(RETRY_BIT | IGNORE_BIT | FAIL_BIT));
    write_opt(b" FF", reply(b'A', 0, true));
    write_opt(b" IG", reply(b'I', IGNORE_BIT, false));
    write_opt(b" RT", reply(b'R', RETRY_BIT, false));
    write_opt(b" AB", reply(b'A', 0, false));
    write_opt(b" FL", reply(b'F', FAIL_BIT, false));
    write_opt(b" Q", reply(b'Q', 0, false));
    crate::bios::mscon::con_write_bytes(b" Y");
    put_hex8(batch_code(end_batch(b'Y')));
    crate::bios::mscon::con_write_bytes(b" N");
    put_hex8(batch_code(end_batch(b'N')));
    crate::bios::mscon::con_write_bytes(b" X");
    put_hex8(batch_code(end_batch(b'X')));
    crate::bios::mscon::con_write_bytes(b" AF");
    put_hex8(u8::from(abort_for(0)));
    crate::bios::mscon::con_write_bytes(b" AF");
    put_hex8(u8::from(abort_for(2)));
    crate::bios::mscon::con_write_bytes(b" AF");
    put_hex8(u8::from(abort_for(1)));
    crate::bios::mscon::con_write_bytes(b" LD");
    put_hex8(u8::from(asks(true, true)));
    crate::bios::mscon::con_write_bytes(b" LD");
    put_hex8(u8::from(asks(false, true)));
    crate::bios::mscon::con_write_bytes(b" LD");
    put_hex8(u8::from(asks(false, false)));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
