//! Destination flush from `CMD/COMMAND/COPYPR1.ASM`.
//!
//! A different name is created. The same name on a device is created too.
//! The same file is an error unless files are being concatenated, in which
//! case the copy only seeks. Seeking uses the write-open flags. Creating
//! uses the create-open flags.

const WRITE_OPEN: u16 = 0x0101;
const CREATE_OPEN: u16 = 0x0112;

fn destination(same: bool, device: bool, concat: bool) -> (u8, u16) {
    if !same || device {
        return (1, CREATE_OPEN);
    }
    if concat {
        return (2, WRITE_OPEN);
    }
    (3, 0)
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"CPR1 ");
    write(destination(false, false, false));
    crate::bios::mscon::con_write_bytes(b" ");
    write(destination(true, true, false));
    crate::bios::mscon::con_write_bytes(b" ");
    write(destination(true, false, true));
    crate::bios::mscon::con_write_bytes(b" ");
    write(destination(true, false, false));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write(action: (u8, u16)) {
    put_hex8(action.0);
    put_hex16(action.1);
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
