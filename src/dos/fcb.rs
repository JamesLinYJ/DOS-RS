//! `MakeFcb` from `DOS/FCB.ASM`.
//!
//! AH=29h in `PARSE.ASM` calls this. AL bit 0 scans one separator, bit 1
//! keeps a drive byte that is already set, and bits 2 and 3 keep a name or
//! extension that is already set. `*` fills the rest of the field with `?`.

use crate::bios::mscon::con_write_bytes;

/// The FCB itself, past an optional `0xFF` extension.
pub(crate) fn split(fcb: *const u8) -> (*const u8, u8) {
    unsafe {
        if !fcb.is_null() && *fcb == 0xff {
            (fcb.add(7), *fcb.add(6))
        } else {
            (fcb, 0)
        }
    }
}

pub(crate) fn split_mut(fcb: *mut u8) -> (*mut u8, u8) {
    let (base, attr) = split(fcb);
    (base as *mut u8, attr)
}

pub(crate) fn packed_name(fcb: *const u8) -> [u8; 11] {
    let (base, _) = split(fcb);
    let mut name = [b' '; 11];
    if base.is_null() {
        return name;
    }
    let mut i = 0usize;
    while i < 11 {
        name[i] = unsafe { *base.add(1 + i) };
        i += 1;
    }
    name
}

/// Scan `text` into `fcb`. The second value is the first byte not consumed.
pub fn make_fcb(flags: u8, text: *const u8, fcb: *mut u8) -> (u8, *const u8) {
    unsafe {
        if flags & 2 == 0 {
            *fcb = 0;
        }
        if flags & 4 == 0 {
            let mut i = 0usize;
            while i < 8 {
                *fcb.add(1 + i) = b' ';
                i += 1;
            }
        }
        if flags & 8 == 0 {
            let mut i = 0usize;
            while i < 3 {
                *fcb.add(9 + i) = b' ';
                i += 1;
            }
        }
        *fcb.add(12) = 0;
        *fcb.add(13) = 0;
        *fcb.add(14) = 0;
        *fcb.add(15) = 0;
    }
    let mut source = text;
    let mut wild = false;
    unsafe {
        loop {
            let result = *source;
            if result != b' ' && result != b'\t' {
                break;
            }
            source = source.add(1);
        }
        if flags & 1 != 0 {
            let result = *source;
            if result == b'.' || result == b',' || result == b';' || result == b'=' || result == b'+' {
                source = source.add(1);
            }
        }
        let result = *source;
        if result > b' ' && *source.add(1) == b':' {
            let drive = up(result).wrapping_sub(b'@');
            if drive == 0 || drive > 26 {
                return (0xff, source);
            }
            *fcb = drive;
            source = source.add(2);
        }
        let mut i = 0usize;
        while i < 8 {
            let result = *source;
            if result == 0 || result == b'.' || result == b' ' || result == b'\r' || result == b'\n' {
                break;
            }
            source = source.add(1);
            if result == b'*' {
                while i < 8 {
                    *fcb.add(1 + i) = b'?';
                    i += 1;
                }
                wild = true;
                break;
            }
            *fcb.add(1 + i) = up(result);
            if result == b'?' {
                wild = true;
            }
            i += 1;
        }
        if *source == b'.' {
            source = source.add(1);
            i = 0;
            while i < 3 {
                let result = *source;
                if result == 0 || result == b' ' || result == b'\r' || result == b'\n' {
                    break;
                }
                source = source.add(1);
                if result == b'*' {
                    while i < 3 {
                        *fcb.add(9 + i) = b'?';
                        i += 1;
                    }
                    wild = true;
                    break;
                }
                *fcb.add(9 + i) = up(result);
                if result == b'?' {
                    wild = true;
                }
                i += 1;
            }
        }
    }
    if wild {
        (1, source)
    } else {
        (0, source)
    }
}

/// Parse, search, open, and read `FOO.COM` through INT 21h.
pub fn show() {
    let text = b"FOO.COM\0";
    let mut fcb = [0u8; 40];
    let parsed = int_parse(text.as_ptr(), fcb.as_mut_ptr());
    if parsed != 0 {
        con_write_bytes(b"FCBFAIL\r\n");
        return;
    }
    let mut dta = [0u8; 128];
    int_dta(dta.as_mut_ptr());
    let mut probe = fcb;
    let sr = int_fcb(0x11, probe.as_mut_ptr());
    let nx = int_fcb(0x12, probe.as_mut_ptr());
    let opened = int_fcb(0x0f, fcb.as_mut_ptr());
    if opened != 0 {
        con_write_bytes(b"FCBFAIL\r\n");
        return;
    }
    let size = u16::from_le_bytes([fcb[16], fcb[17]]);
    let _len = int_fcb(0x23, fcb.as_mut_ptr());
    let result = int_fcb(0x14, fcb.as_mut_ptr());
    let _closed = int_fcb(0x10, fcb.as_mut_ptr());
    con_write_bytes(b"FCB FOO.COM ");
    put_hex16(size);
    con_write_bytes(b" SR");
    put_hex8(sr);
    con_write_bytes(b" NX");
    put_hex8(nx);
    con_write_bytes(b" AL");
    put_hex8(result);
    con_write_bytes(b"\r\n");
}

fn int_parse(text: *const u8, fcb: *mut u8) -> u8 {
    let (result, _) = crate::dos::parse::parse(0, text, fcb);
    result
}

fn int_fcb(function: u8, fcb: *mut u8) -> u8 {
    match function {
        0x0F => crate::dos::fcbio2::open(fcb),
        0x10 => crate::dos::fcbio::close(fcb),
        0x11 => crate::dos::search::fcb_search_first(fcb),
        0x12 => crate::dos::search::fcb_search_next(),
        0x14 => crate::dos::fcbio2::seq_read(fcb),
        0x23 => crate::dos::fcbio::file_length(fcb),
        _ => 0xff,
    }
}

fn int_dta(dta: *mut u8) {
    crate::dos::search::set_dta(dta);
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    con_write_bytes(&[hex[(value >> 4) as usize], hex[(value & 0x0f) as usize]]);
}

fn up(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte - 0x20
    } else {
        byte
    }
}
