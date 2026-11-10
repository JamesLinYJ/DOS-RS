//! `CATALOG` from `CMD/COMMAND/TCMD1A.ASM`.
//!
//! A blank `DIR` lists the root of the boot disk. The header is message
//! 1048. Each name is the 8-byte field, a blank, and the 3-byte extension.

use crate::bios::mscon::con_write_bytes;

const IMAGE: &[u8] = include_bytes!("../../../disks/msload.img");

pub fn dir_cmd(tail: &[u8]) {
    if !blank(tail) {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    con_write_bytes(b"Directory of  A:\\\r\n");
    let root = 3 * 512;
    let mut index = 0usize;
    while index < 16 {
        let entry = &IMAGE[root + index * 32..root + index * 32 + 32];
        if entry[0] == 0 {
            break;
        }
        if entry[0] != 0xe5 && entry[11] & 0x08 == 0 {
            con_write_bytes(&entry[..8]);
            con_write_bytes(b" ");
            con_write_bytes(&entry[8..11]);
            con_write_bytes(b"\r\n");
        }
        index += 1;
    }
}

fn blank(tail: &[u8]) -> bool {
    for &byte in tail {
        if byte == b'\r' || byte == b'\n' || byte == 0 {
            return true;
        }
        if byte != b' ' && byte != b'\t' {
            return false;
        }
    }
    true
}
