//! Country data and case mapping from `DOS/DOSMES.ASM`.
//!
//! The assembled word is code page 437, with six entries and USA as country 1.
//! Characters below 80h are unchanged. 82h maps through the upper-case table to
//! E. A file character is included, then excluded through 20h, then excluded
//! if it is one of the fourteen specials. DBCS ranges are empty. The standard
//! escape table uses S for copy-one. The IBM table uses 77.

const CODE_PAGE: u16 = 437;
const ENTRIES: u8 = 6;
const COUNTRY: u16 = 1;
const DATE_FORMAT: u8 = 0;
const CURRENCY: u8 = b'$';
const THOUSAND: u8 = b',';
const DECIMAL: u8 = b'.';
const SPECIAL: &[u8] = b".\"/\\[]:|<>+=;,";

const UPPER: [u8; 128] = [
    128, 154, 69, 65, 142, 65, 143, 128, 69, 69, 69, 73, 73, 73, 142, 143, 144, 146, 146, 79,
    153, 79, 85, 85, 89, 153, 154, 155, 156, 157, 158, 159, 65, 73, 79, 85, 165, 165, 166, 167,
    168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185,
    186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203,
    204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221,
    222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239,
    240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254, 255,
];

const COLLATE_HIGH: [u8; 128] = [
    67, 85, 69, 65, 65, 65, 65, 67, 69, 69, 69, 73, 73, 73, 65, 65, 69, 65, 65, 79, 79, 79, 85,
    85, 89, 79, 85, 36, 36, 36, 36, 36, 65, 73, 79, 85, 78, 78, 166, 167, 63, 169, 170, 171, 172,
    33, 34, 34, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191,
    192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210,
    211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 83, 226, 227, 228, 229,
    230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248,
    249, 250, 251, 252, 253, 254, 255,
];

/// OEM number. An IBM build with the copyright flag stores 0. Every other
/// combination stores FF.
pub fn oem_number(ibm: bool, copyright: bool) -> u8 {
    if ibm && copyright { 0 } else { 0xff }
}

/// `MAP_CASE`. Bytes below 80h are returned unchanged.
pub fn map_case(byte: u8) -> u8 {
    if byte < 0x80 {
        return byte;
    }
    UPPER[(byte - 0x80) as usize]
}

/// Collate weight. The letters a through z weigh the same as A through Z.
pub fn collate(byte: u8) -> u8 {
    if byte < 0x80 {
        if byte.is_ascii_lowercase() {
            return byte - (b'a' - b'A');
        }
        return byte;
    }
    COLLATE_HIGH[(byte - 0x80) as usize]
}

/// Valid file character. Include everything, drop 00h through 20h, then drop
/// the fourteen specials.
pub fn file_char_ok(byte: u8) -> bool {
    if byte <= 0x20 {
        return false;
    }
    let mut index = 0usize;
    while index < SPECIAL.len() {
        if byte == SPECIAL[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// DBCS pair count. The table length word is 0.
pub fn dbcs_pairs() -> u16 {
    0
}

/// Second byte that means copy-one. IBM is 77, Wang is C1, and the standard
/// table uses S.
pub fn copy_one_lead(ibm: bool, wang: bool) -> u8 {
    if ibm {
        77
    } else if wang {
        0xc1
    } else {
        b'S'
    }
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DOSMES OEM");
    put_hex8(oem_number(false, false));
    crate::bios::mscon::con_write_bytes(b" IB");
    put_hex8(oem_number(true, true));
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex16(CODE_PAGE);
    crate::bios::mscon::con_write_bytes(b" ENT");
    put_hex8(ENTRIES);
    crate::bios::mscon::con_write_bytes(b" US");
    put_hex16(COUNTRY);
    crate::bios::mscon::con_write_bytes(b" DT");
    put_hex8(DATE_FORMAT);
    crate::bios::mscon::con_write_bytes(b" CUR");
    put_hex8(CURRENCY);
    crate::bios::mscon::con_write_bytes(b" TH");
    put_hex8(THOUSAND);
    crate::bios::mscon::con_write_bytes(b" DEC");
    put_hex8(DECIMAL);
    crate::bios::mscon::con_write_bytes(b" DB");
    put_hex16(dbcs_pairs());
    crate::bios::mscon::con_write_bytes(b" MA");
    put_hex8(map_case(b'A'));
    crate::bios::mscon::con_write_bytes(b" M82");
    put_hex8(map_case(0x82));
    crate::bios::mscon::con_write_bytes(if file_char_ok(b'A') { b" FA01" } else { b" FA00" });
    crate::bios::mscon::con_write_bytes(if file_char_ok(0x20) { b" FS01" } else { b" FS00" });
    crate::bios::mscon::con_write_bytes(if file_char_ok(b'.') { b" FD01" } else { b" FD00" });
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(collate(b'a'));
    crate::bios::mscon::con_write_bytes(b" CH");
    put_hex8(collate(128));
    crate::bios::mscon::con_write_bytes(b" ES");
    put_hex8(copy_one_lead(false, false));
    crate::bios::mscon::con_write_bytes(b" IB");
    put_hex8(copy_one_lead(true, false));
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
