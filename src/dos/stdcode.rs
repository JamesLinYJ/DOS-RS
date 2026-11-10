//! Standard absolute disk calls from `DOS/STDCODE.ASM`.
//!
//! `IBM` is false, so the `ERRIN` translation is not assembled. The BIOS
//! status is returned as it arrived. Drive 255 is still rejected by `GETBP`.

const IBM: u16 = 0;

pub fn ancient(status: u8) -> u8 {
    status
}

pub fn drive_ok(unit: u8) -> bool {
    unit != 0xff
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"STDCODE IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" MAP02");
    put_hex8(ancient(2));
    crate::bios::mscon::con_write_bytes(b" MAP06");
    put_hex8(ancient(6));
    crate::bios::mscon::con_write_bytes(if drive_ok(0) { b" DRV00 OK" } else { b" DRV00 BAD" });
    crate::bios::mscon::con_write_bytes(if drive_ok(0xff) { b" DRVFF OK" } else { b" DRVFF BAD" });
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
