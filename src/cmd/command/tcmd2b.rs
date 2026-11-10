//! `CTTY`, `CHCP`, and `TRUENAME` from `CMD/COMMAND/TCMD2B.ASM`.
//!
//! `CTTY` accepts only a device, and it writes a return before adopting it.
//! `CHCP` with no number displays the page. A missing country file is not the
//! same as an unknown page. `TRUENAME` with no name asks for the current
//! directory. A permanent command does not restore the critical messages.

const FILE_NOT_FOUND: u16 = 2;
const INVALID_DATA: u16 = 13;
const ACCESS_DENIED: u16 = 65;
const NLS_PRESENT: u8 = 0xFF;
const SHOW: u8 = 0;
const SET_OK: u8 = 1;
const MISSING_NLS: u8 = 2;
const INVALID_PAGE: u8 = 3;
const MISSING_COUNTRY: u8 = 4;
const NOT_ALL: u8 = 5;
const NOT_SET: u8 = 6;
const PARSE_ERROR: u8 = 7;

fn end_of_line(code: u16) -> bool {
    code as u8 == 0xFF
}

fn ctty(ended: bool, opened: bool, attributes: u8, wrote: bool) -> Option<u8> {
    if ended || !opened {
        return None;
    }
    if attributes & 0x80 == 0 {
        return None;
    }
    if !wrote {
        return None;
    }
    Some(attributes | 3)
}

fn chcp(ended: bool, parsed: bool, nls: u8, set_error: Option<u16>, extended: u16) -> u8 {
    if ended {
        return SHOW;
    }
    if !parsed {
        return PARSE_ERROR;
    }
    if nls != NLS_PRESENT {
        return MISSING_NLS;
    }
    let Some(error) = set_error else {
        return SET_OK;
    };
    if error == FILE_NOT_FOUND {
        if extended == INVALID_DATA {
            INVALID_PAGE
        } else {
            MISSING_COUNTRY
        }
    } else if extended == ACCESS_DENIED {
        NOT_ALL
    } else {
        NOT_SET
    }
}

fn truename_current() -> [u8; 2] {
    [b'.', 0]
}

fn truename_drive(unit: u8) -> [u8; 4] {
    [unit.wrapping_add(b'A' - 1), b':', b'.', 0]
}

fn exit_restores(permanent: bool) -> bool {
    !permanent
}

fn write_opt(label: &[u8], value: Option<u8>) {
    crate::bios::mscon::con_write_bytes(label);
    put_hex8(value.unwrap_or(0));
}

pub fn show() {
    let current = truename_current();
    let drive_a = truename_drive(1);
    let drive_c = truename_drive(3);
    crate::bios::mscon::con_write_bytes(b"TCMD2B ");
    write_opt(b"CT", ctty(true, true, 0x80, true));
    write_opt(b" CT", ctty(false, false, 0x80, true));
    write_opt(b" CT", ctty(false, true, 0, true));
    write_opt(b" CT", ctty(false, true, 0x80, false));
    write_opt(b" CT", ctty(false, true, 0x80, true));
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex8(chcp(true, true, NLS_PRESENT, None, 0));
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex8(chcp(false, false, NLS_PRESENT, None, 0));
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex8(chcp(false, true, 0, None, 0));
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex8(chcp(false, true, NLS_PRESENT, None, 0));
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex8(chcp(false, true, NLS_PRESENT, Some(2), 13));
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex8(chcp(false, true, NLS_PRESENT, Some(2), 2));
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex8(chcp(false, true, NLS_PRESENT, Some(5), 65));
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex8(chcp(false, true, NLS_PRESENT, Some(5), 1));
    crate::bios::mscon::con_write_bytes(b" TN");
    put_hex8(current[0]);
    put_hex8(current[1]);
    crate::bios::mscon::con_write_bytes(b" TN");
    put_hex8(drive_a[0]);
    put_hex8(drive_a[1]);
    put_hex8(drive_a[2]);
    crate::bios::mscon::con_write_bytes(b" TN");
    put_hex8(drive_c[0]);
    put_hex8(drive_c[1]);
    put_hex8(drive_c[2]);
    crate::bios::mscon::con_write_bytes(b" TN");
    put_hex8(b'F');
    put_hex8(b'O');
    put_hex8(b'O');
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(u8::from(exit_restores(true)));
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(u8::from(exit_restores(false)));
    crate::bios::mscon::con_write_bytes(b" EL");
    put_hex8(u8::from(end_of_line(0xFFFF)));
    crate::bios::mscon::con_write_bytes(b" EL");
    put_hex8(u8::from(end_of_line(0)));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
