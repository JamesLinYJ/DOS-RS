//! String and dispatch helpers from `DOS/MISC2.ASM`.
//!
//! `StrCmp` and `StrCpy` uppercase and treat `/` as `\`. Lengths include the
//! null. A table index at or past the limit is invalid. A null CDS is a
//! network. Idle returns at once when sharing is on. `FastRet` fails with
//! `FFFFh`. An odd message number selects the same slot as the even get.

const FAST_YES: u8 = 0x80;
const NET_CDS: u16 = 0x8000;
const NET_FILE: u16 = 0x8000;

/// `uCase` through `GetLet2`. Letters fold, then bytes at 80h use the country table.
pub fn upper(byte: u8) -> u8 {
    let folded = if byte.is_ascii_lowercase() {
        byte - (b'a' - b'A')
    } else {
        byte
    };
    super::dosmes::map_case(folded)
}

fn normalize(byte: u8) -> u8 {
    super::macro2::path_char(upper(byte)).0
}

/// `StrCmp`. Different lengths do not match.
pub fn same_text(left: &[u8], right: &[u8]) -> bool {
    let mut index = 0usize;
    loop {
        let left_byte = normalize(left.get(index).copied().unwrap_or(0));
        let right_byte = normalize(right.get(index).copied().unwrap_or(0));
        if left_byte != right_byte {
            return false;
        }
        if left_byte == 0 {
            return true;
        }
        index += 1;
    }
}

/// `StrCpy`. The copy includes the terminating null.
pub fn copy_text(source: &[u8], dest: &mut [u8]) -> usize {
    let mut index = 0usize;
    loop {
        let byte = normalize(source.get(index).copied().unwrap_or(0));
        if index < dest.len() {
            dest[index] = byte;
        }
        index += 1;
        if byte == 0 {
            return index;
        }
    }
}

/// `FStrCpy`. Bytes are copied unchanged, including the null.
pub fn copy_raw(source: &[u8], dest: &mut [u8]) -> usize {
    let mut index = 0usize;
    loop {
        let byte = source.get(index).copied().unwrap_or(0);
        if index < dest.len() {
            dest[index] = byte;
        }
        index += 1;
        if byte == 0 {
            return index;
        }
    }
}

/// `StrLen`, including the null.
pub fn text_len(text: &[u8]) -> u16 {
    let mut index = 0usize;
    loop {
        let byte = text.get(index).copied().unwrap_or(0);
        index += 1;
        if byte == 0 {
            return index as u16;
        }
    }
}

/// `TableDispatch`. An index greater than or equal to the limit is invalid.
pub fn table_slot(index: u8, limit: u8) -> Result<u8, ()> {
    if index >= limit {
        return Err(());
    }
    Ok(index)
}

/// `TestNet`. A null CDS or the network flag means network.
pub fn is_network(null_cds: bool, flags: u16) -> bool {
    if null_cds {
        return true;
    }
    if flags & NET_CDS != 0 {
        return true;
    }
    false
}

/// `IsSFTNet`. The network bit clears zero. Carry stays clear.
pub fn file_is_network(flags: u16) -> bool {
    flags & NET_FILE != 0
}

/// `Idle`. Sharing skips the spin. Otherwise the retry count is the spin count.
pub fn idle_spins(sharing: bool, retries: u16) -> u16 {
    if sharing {
        return 0;
    }
    retries
}

/// `FastInit`. A foreign resident segment is already installed. Query does not store.
pub fn fast_install(resident: u16, dos_segment: u16, query: bool) -> Result<u8, ()> {
    if resident != dos_segment && resident != 0 {
        return Err(());
    }
    if query {
        return Ok(0);
    }
    Ok(FAST_YES)
}

/// `FastRet`. The placeholder always fails.
pub fn fast_absent() -> u16 {
    0xffff
}

/// `Fake_version`. Zero keeps the current version. Any other word is special.
pub fn special_version(value: u16) -> u16 {
    value
}

/// `MSG_RETRIEVAL`. Odd numbers address the same dword as the preceding get.
pub fn message_slot(which: u8) -> u16 {
    let mut index = which;
    if index & 1 != 0 {
        index -= 1;
    }
    u16::from(index) << 1
}

pub fn show() {
    let equal = same_text(b"A/b", b"a\\B");
    let different = same_text(b"A", b"AB");
    let mut copied = [0u8; 8];
    let _ = copy_text(b"a/b", &mut copied);
    let mut raw = [0u8; 4];
    let _ = copy_raw(b"a/b", &mut raw);
    let length = text_len(b"ab");
    crate::bios::mscon::con_write_bytes(if equal { b"MISC2 EQ1" } else { b"MISC2 EQ0" });
    crate::bios::mscon::con_write_bytes(if different { b" NE1" } else { b" NE0" });
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex8(copied[0]);
    put_hex8(copied[1]);
    put_hex8(copied[2]);
    crate::bios::mscon::con_write_bytes(b" RAW");
    put_hex8(raw[0]);
    crate::bios::mscon::con_write_bytes(b" LN");
    put_hex8(length as u8);
    crate::bios::mscon::con_write_bytes(b" UC");
    put_hex8(upper(b'a'));
    crate::bios::mscon::con_write_bytes(b" U82");
    put_hex8(upper(0x82));
    crate::bios::mscon::con_write_bytes(match table_slot(0, 5) {
        Ok(_) => b" TDOK",
        Err(()) => b" TDBAD",
    });
    crate::bios::mscon::con_write_bytes(match table_slot(5, 5) {
        Ok(_) => b" TD05OK",
        Err(()) => b" TD05BAD",
    });
    crate::bios::mscon::con_write_bytes(if is_network(true, 0) { b" NET1" } else { b" NET0" });
    crate::bios::mscon::con_write_bytes(if is_network(false, 0) { b" LOC1" } else { b" LOC0" });
    crate::bios::mscon::con_write_bytes(if file_is_network(NET_FILE) {
        b" SNET1"
    } else {
        b" SNET0"
    });
    crate::bios::mscon::con_write_bytes(b" ID");
    put_hex8(idle_spins(true, 3) as u8);
    crate::bios::mscon::con_write_bytes(b" ID");
    put_hex8(idle_spins(false, 3) as u8);
    crate::bios::mscon::con_write_bytes(b" FV");
    put_hex16(special_version(0));
    crate::bios::mscon::con_write_bytes(b" FV");
    put_hex16(special_version(0x1234));
    crate::bios::mscon::con_write_bytes(b" MSG");
    put_hex16(message_slot(0));
    crate::bios::mscon::con_write_bytes(b" MSG");
    put_hex16(message_slot(3));
    crate::bios::mscon::con_write_bytes(b" FS");
    put_hex16(fast_absent());
    crate::bios::mscon::con_write_bytes(match fast_install(0x1234, 0x0070, false) {
        Err(()) => b" FIA",
        Ok(_) => b" FIOK",
    });
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
