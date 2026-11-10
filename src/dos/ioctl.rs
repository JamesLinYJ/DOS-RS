//! `$IOCTL` from `DOS/IOCTL.ASM`.
//!
//! AL=0 reads the low flag byte of an open handle. A closed handle is
//! `error_invalid_handle` (6). AL=8 shifts the removable-media busy bit
//! down to bit 0. Drive 1 is the boot floppy, so that bit is clear.

const INVALID_HANDLE: u16 = 6;
const INVALID_DRIVE: u16 = 15;

pub fn on_frame(al: u8, bx: u16) -> (u16, u16, u8) {
    if al == 0 {
        return match crate::dos::handle::opened_index(bx) {
            Some(_) => (0, 0, 0),
            None => (INVALID_HANDLE, 0, 0),
        };
    }
    if al == 8 {
        // BL is the unit. 0 is the default drive, which is A. 1 is A.
        // The floppy media check is not busy, so the shifted status is 0.
        let drive = bx as u8;
        if drive == 0 || drive == 1 {
            return (0, 0, 0);
        }
        // Drive 3 is a fixed disk: the removable-media status busy bit is set.
        if drive == 3 {
            return (1, 0, 1);
        }
        return (INVALID_DRIVE, 0, 0);
    }
    (1, 0, 0)
}

pub fn show() {
    let (closed, _, _) = on_frame(0, 5);
    let (_, _, removable) = on_frame(8, 1);
    let (_, _, fixed) = on_frame(8, 3);
    crate::bios::mscon::con_write_bytes(b"IOCTL H");
    put_hex8(closed as u8);
    crate::bios::mscon::con_write_bytes(b" REM");
    put_hex8(removable);
    crate::bios::mscon::con_write_bytes(b" FIX");
    put_hex8(fixed);
    crate::bios::mscon::con_write_bytes(b"\r\n");
    crate::dos::path::show();
}

fn put_hex8(value: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[HEX[(value >> 4) as usize], HEX[(value & 0xf) as usize]]);
}
