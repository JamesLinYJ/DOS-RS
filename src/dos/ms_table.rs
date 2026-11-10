//! Tables from `DOS/MS_TABLE.ASM`.
//!
//! The INT 21h call vectors stay in `src/int21.rs`. This is the allowed-error
//! walk and the version banner. A call missing from the table keeps its error.
//! A call that is present keeps an allowed code and otherwise returns the last
//! code in that row. The assembled IBM table has an empty banner. The MS-DOS
//! banner is version 4.00, and high memory appends `H`.

const MAJOR: u8 = 4;
const MINOR: u8 = 0;
const FAST_OPEN_ENTRIES: u16 = 2;
const SWAP_AREAS: u16 = 2;
const NO_NAME: &[u8] = b"NO NAME    ";
const CACHE_DRIVE: u8 = 0xff;
const LAST_SECTOR: u32 = 0xffff_ffff;
const FETCH_TAG: u16 = 0;
const SPECIAL_VERSION: u16 = 0;

struct Allowed {
    call: u8,
    errors: &'static [u8],
}

const ALLOWED: &[Allowed] = &[
    Allowed { call: 56, errors: &[1, 2] },
    Allowed { call: 57, errors: &[3, 2, 5] },
    Allowed { call: 58, errors: &[16, 3, 2, 5] },
    Allowed { call: 59, errors: &[2, 3] },
    Allowed { call: 60, errors: &[3, 2, 4, 5] },
    Allowed { call: 61, errors: &[3, 2, 12, 4, 26, 5] },
    Allowed { call: 62, errors: &[6] },
    Allowed { call: 63, errors: &[6, 5] },
    Allowed { call: 64, errors: &[6, 5] },
    Allowed { call: 65, errors: &[3, 2, 5] },
    Allowed { call: 66, errors: &[6, 1] },
    Allowed { call: 67, errors: &[3, 2, 1, 5] },
    Allowed { call: 68, errors: &[15, 13, 1, 6, 5] },
    Allowed { call: 69, errors: &[6, 4] },
    Allowed { call: 70, errors: &[6, 4] },
    Allowed { call: 71, errors: &[26, 15] },
    Allowed { call: 72, errors: &[7, 8] },
    Allowed { call: 73, errors: &[7, 9] },
    Allowed { call: 74, errors: &[7, 9, 8] },
    Allowed { call: 75, errors: &[3, 1, 2, 4, 11, 10, 8, 5] },
    Allowed { call: 78, errors: &[3, 2, 18] },
    Allowed { call: 79, errors: &[18] },
    Allowed { call: 86, errors: &[17, 3, 2, 16, 5] },
    Allowed { call: 87, errors: &[6, 8, 13, 1] },
    Allowed { call: 88, errors: &[1] },
    Allowed { call: 90, errors: &[3, 2, 4, 5] },
    Allowed { call: 91, errors: &[80, 3, 2, 4, 5] },
    Allowed { call: 92, errors: &[6, 1, 36, 33] },
    Allowed { call: 101, errors: &[1, 2] },
    Allowed { call: 102, errors: &[1, 2] },
    Allowed { call: 104, errors: &[6] },
    Allowed { call: 103, errors: &[4, 8, 1] },
    Allowed {
        call: 108,
        errors: &[3, 2, 12, 4, 80, 8, 26, 13, 1, 5],
    },
    Allowed { call: 105, errors: &[15, 13, 1, 5] },
    Allowed { call: 0xff, errors: &[] },
];

fn version_word() -> u16 {
    u16::from(MAJOR) | (u16::from(MINOR) << 8)
}

/// `ETAB_LK`. The end marker is `FF`. The last code in a row is the fallback.
pub fn map_error(call: u8, error: u8) -> u8 {
    let mut index = 0usize;
    while index < ALLOWED.len() {
        let entry = &ALLOWED[index];
        if entry.call == 0xff {
            return error;
        }
        if entry.call == call {
            let mut which = 0usize;
            while which < entry.errors.len() {
                if entry.errors[which] == error {
                    return error;
                }
                which += 1;
            }
            return entry.errors[entry.errors.len() - 1];
        }
        index += 1;
    }
    error
}

fn push(out: &mut [u8], mut used: usize, bytes: &[u8]) -> usize {
    let mut index = 0usize;
    while index < bytes.len() {
        out[used] = bytes[index];
        used += 1;
        index += 1;
    }
    used
}

fn push_version(out: &mut [u8], used: usize) -> usize {
    let text = [
        MAJOR + b'0',
        b'.',
        (MINOR / 10) + b'0',
        (MINOR % 10) + b'0',
    ];
    push(out, used, &text)
}

/// Banner bytes. Debug text, then the MS-DOS line when this is not IBM.
pub fn banner(ibm: bool, debug: bool, high_memory: bool, out: &mut [u8]) -> usize {
    let mut used = 0usize;
    if debug {
        used = push(out, used, b"\r\nDebugging DOS version ");
        used = push_version(out, used);
    }
    if !ibm {
        used = push(out, used, b"\r\nMS-DOS version ");
        used = push_version(out, used);
        if high_memory {
            used = push(out, used, b"H");
        }
        used = push(
            out,
            used,
            b"\r\nCopyright 1981,82,83,84,88 Microsoft Corp.\r\n$",
        );
    }
    if debug {
        used = push(out, used, b"\r\n$");
    }
    used
}

fn trailing_spaces(text: &[u8]) -> u8 {
    let mut count = 0u8;
    let mut index = text.len();
    while index != 0 {
        index -= 1;
        if text[index] != b' ' {
            break;
        }
        count += 1;
    }
    count
}

pub fn show() {
    let mut text = [0u8; 128];
    let ibm = banner(true, false, false, &mut text);
    let debug_only = banner(true, true, false, &mut text);
    let ms = banner(false, false, false, &mut text);
    let ms_mark = text[22];
    let high = banner(false, false, true, &mut text);
    let high_mark = text[22];

    crate::bios::mscon::con_write_bytes(b"MSTAB VR");
    put_hex16(version_word());
    crate::bios::mscon::con_write_bytes(b" MAP");
    put_hex8(0x3d);
    put_hex8(map_error(0x3d, 2));
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(0x3d);
    put_hex8(map_error(0x3d, 32));
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(0x3b);
    put_hex8(map_error(0x3b, 2));
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(0x3b);
    put_hex8(map_error(0x3b, 5));
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(0x01);
    put_hex8(map_error(0x01, 0x63));
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(0x3e);
    put_hex8(map_error(0x3e, 5));
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(0x4f);
    put_hex8(map_error(0x4f, 2));
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(0x6c);
    put_hex8(map_error(0x6c, 0x50));
    crate::bios::mscon::con_write_bytes(b" BN");
    put_hex16(ibm as u16);
    crate::bios::mscon::con_write_bytes(b" BN");
    put_hex16(debug_only as u16);
    crate::bios::mscon::con_write_bytes(b" BN");
    put_hex16(ms as u16);
    put_hex8(ms_mark);
    crate::bios::mscon::con_write_bytes(b" BN");
    put_hex16(high as u16);
    put_hex8(high_mark);
    crate::bios::mscon::con_write_bytes(b" FO");
    put_hex16(FAST_OPEN_ENTRIES);
    crate::bios::mscon::con_write_bytes(b" NS");
    put_hex16(SWAP_AREAS);
    crate::bios::mscon::con_write_bytes(b" NM");
    put_hex16(NO_NAME.len() as u16);
    crate::bios::mscon::con_write_bytes(b" SP");
    put_hex8(trailing_spaces(NO_NAME));
    crate::bios::mscon::con_write_bytes(b" SC");
    put_hex8(CACHE_DRIVE);
    crate::bios::mscon::con_write_bytes(b" SQ");
    put_hex16((LAST_SECTOR >> 16) as u16);
    put_hex16(LAST_SECTOR as u16);
    crate::bios::mscon::con_write_bytes(b" TG");
    put_hex16(FETCH_TAG);
    crate::bios::mscon::con_write_bytes(b" SV");
    put_hex16(SPECIAL_VERSION);
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
