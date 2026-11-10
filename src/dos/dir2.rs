//! Path cracking from `DOS/DIR2.ASM`.
//!
//! `DEVNAME` walks the character-device list. `NUL` is the head and links to
//! `CON`. A volume-label search never matches a device. `MatchAttributes`
//! keeps a found attribute only when every important bit was requested.
//! `StartSrch` clears the entry cursors, and an empty root path is a directory.

const ATTR_VOLUME: u8 = 0x08;
const ATTR_ALL: u8 = 0x02 + 0x04 + 0x10;
const ATTR_DEVICE: u8 = 0x40;
const DEV_CHAR: u16 = 0x8000;

struct Device {
    name: [u8; 8],
    attr: u16,
    next: isize,
}

/// `NULDEV` then the console chain. The last header is a block device.
const DEVICES: [Device; 6] = [
    Device { name: *b"NUL     ", attr: 0x8004, next: 1 },
    Device { name: *b"CON     ", attr: 0x8003, next: 2 },
    Device { name: *b"AUX     ", attr: 0x8000, next: 3 },
    Device { name: *b"PRN     ", attr: 0x8000, next: 4 },
    Device { name: *b"CLOCK$  ", attr: 0x8008, next: 5 },
    Device { name: *b"        ", attr: 0x0000, next: -1 },
];

pub struct Srch {
    pub last_ent: u16,
    pub ent_free: u16,
    #[allow(dead_code)]
    pub ent_last: u16,
    #[allow(dead_code)]
    pub vol_id: u8,
}

/// `StartSrch`. The free and last slots are -1.
pub fn start_srch() -> Srch {
    Srch {
        last_ent: 0,
        ent_free: 0xffff,
        ent_last: 0xffff,
        vol_id: 0,
    }
}

/// `MatchAttributes`. Zero means the found attribute is allowed.
pub fn match_attributes(attrib: u8, found: u8) -> bool {
    let result = (!attrib) & found & ATTR_ALL;
    result == 0
}

/// `DEVNAME`. `Some` is BH. Volume searches and unknown names miss.
pub fn dev_name(name: &mut [u8; 11], attrib: u8) -> Option<u8> {
    let saved = u16::from_le_bytes([name[0], name[1]]);
    if name[0] == 0x05 {
        name[0] = 0xe5;
    }
    let found = if attrib & ATTR_VOLUME != 0 {
        None
    } else {
        walk(name)
    };
    let bytes = saved.to_le_bytes();
    name[0] = bytes[0];
    name[1] = bytes[1];
    found
}

fn walk(name: &[u8; 11]) -> Option<u8> {
    let mut index = 0isize;
    while index >= 0 {
        let dev = &DEVICES[index as usize];
        if dev.attr & DEV_CHAR != 0 && dev.name == name[..8] {
            let high_byte = ((dev.attr as u8) | 0xc0) & !0x20;
            return Some(high_byte);
        }
        index = dev.next;
    }
    None
}

/// `ChkDev`. A dot or a NUL ends the name. A slash means it is a path.
pub fn chk_dev(text: &[u8], attrib: u8) -> Option<u8> {
    let mut name = [b' '; 11];
    let mut stored = 0usize;
    let mut count = 9usize;
    let mut source = 0usize;
    loop {
        if count == 0 {
            return None;
        }
        let result = if source >= text.len() {
            0
        } else {
            let byte = up(text[source]);
            source += 1;
            byte
        };
        if result == b'.' || result == 0 {
            let pad = count + 2;
            let mut n = 0usize;
            while n < pad && stored < 11 {
                name[stored] = b' ';
                stored += 1;
                n += 1;
            }
            return dev_name(&mut name, attrib);
        }
        if result == b'\\' || result == b'/' {
            return None;
        }
        if stored < 11 {
            name[stored] = result;
            stored += 1;
        }
        count -= 1;
    }
}

/// `Build_device_ent`. The extension is blank and the attribute is the device bit.
pub fn build_device_ent(name: &[u8; 11]) -> [u8; 32] {
    let mut ent = [0u8; 32];
    ent[..8].copy_from_slice(&name[..8]);
    ent[8] = b' ';
    ent[9] = b' ';
    ent[10] = b' ';
    ent[11] = ATTR_DEVICE;
    ent
}

/// `ROOTPATH` when the string after `d:\` is empty. AH is 0 and the zero flag is set.
pub fn root_path(sattrib: u8) -> (u8, u8, bool) {
    let attrib = sattrib;
    let function = 0u8;
    (function, attrib, true)
}

/// `GETPATH` for a canonical `d:\` path or a `d:/` device.
pub fn get_path(wfp: &[u8], sattrib: u8) -> Result<u8, ()> {
    if wfp.len() >= 3 && wfp[1] == b':' && wfp[2] == b'\\' {
        if wfp.len() == 3 || wfp[3] == 0 {
            let (function, _, zero) = root_path(sattrib);
            if zero && function == 0 {
                return Ok(0);
            }
        }
        return Err(());
    }
    let rest = if wfp.len() > 3 { &wfp[3..] } else { b"" };
    match chk_dev(rest, 0) {
        Some(high_byte) => Ok(high_byte),
        None => Err(()),
    }
}

pub fn show() {
    let con = chk_dev(b"CON", 0);
    let nul = chk_dev(b"NUL", 0);
    let sample = chk_dev(b"FOO.COM", 0);
    let vol = chk_dev(b"CON", ATTR_VOLUME);
    let dir_ok = match_attributes(0x10, 0x10);
    let file_ok = match_attributes(0, 0x10);
    let srch = start_srch();
    let (root_ah, _, root_zero) = root_path(0x16);
    let mut con_name = [b' '; 11];
    con_name[..3].copy_from_slice(b"CON");
    let ent = build_device_ent(&con_name);
    crate::bios::mscon::con_write_bytes(b"DIR2 CON");
    put_hex8(con.unwrap_or(0));
    crate::bios::mscon::con_write_bytes(b" NUL");
    put_hex8(nul.unwrap_or(0));
    crate::bios::mscon::con_write_bytes(if sample.is_none() { b" ND" } else { b" DV" });
    crate::bios::mscon::con_write_bytes(if vol.is_none() { b" VOLND" } else { b" VOLDV" });
    crate::bios::mscon::con_write_bytes(b" MAT");
    crate::bios::mscon::con_write_bytes(if file_ok { b"1" } else { b"0" });
    crate::bios::mscon::con_write_bytes(if dir_ok { b"1" } else { b"0" });
    crate::bios::mscon::con_write_bytes(b" RT");
    put_hex8(root_ah);
    crate::bios::mscon::con_write_bytes(if root_zero { b" SR" } else { b" NZ SR" });
    put_hex16(srch.last_ent);
    put_hex16(srch.ent_free);
    crate::bios::mscon::con_write_bytes(b" ENT");
    put_hex8(ent[11]);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn up(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte - (b'a' - b'A')
    } else {
        byte
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
