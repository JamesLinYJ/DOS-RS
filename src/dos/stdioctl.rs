//! Standard IOCTL build from `DOS/STDIOCTL.ASM`.
//!
//! That file sets `IBM` false and includes the same `INC/IOCTL.INC` as the
//! IBM wrapper. The function numbers do not change. The switch is the
//! difference.

const IBM: u16 = 0;
const GET_DEVICE_INFO: u8 = 0;
const SET_DEVICE_INFO: u8 = 1;
const GENERIC_IOCTL: u8 = 0x0d;
const GET_MEDIA_ID: u8 = 0x66;
const MAX_SECTORS_IN_TRACK: u8 = 63;

pub struct Build {
    pub ibm: u16,
    pub get_info: u8,
    pub set_info: u8,
    pub generic: u8,
    pub media_id: u8,
    pub sectors: u8,
}

pub fn build() -> Build {
    Build {
        ibm: IBM,
        get_info: GET_DEVICE_INFO,
        set_info: SET_DEVICE_INFO,
        generic: GENERIC_IOCTL,
        media_id: GET_MEDIA_ID,
        sectors: MAX_SECTORS_IN_TRACK,
    }
}

pub fn show() {
    let chosen = build();
    crate::bios::mscon::con_write_bytes(b"STDIOCTL IBM");
    put_hex16(chosen.ibm);
    crate::bios::mscon::con_write_bytes(b" GET");
    put_hex8(chosen.get_info);
    crate::bios::mscon::con_write_bytes(b" SET");
    put_hex8(chosen.set_info);
    crate::bios::mscon::con_write_bytes(b" GEN");
    put_hex8(chosen.generic);
    crate::bios::mscon::con_write_bytes(b" MED");
    put_hex8(chosen.media_id);
    crate::bios::mscon::con_write_bytes(b" SEC");
    put_hex8(chosen.sectors);
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
