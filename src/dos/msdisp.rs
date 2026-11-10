//! IBM system-call dispatch from `DOS/MSDISP.ASM`.
//!
//! `MSSW.ASM` leaves `IBM` true, so there is no OEM hook. `CALL 5` accepts
//! only functions through `MAXCALL` (`24h`). `INT 21h` accepts through
//! `MAXCOM` (`6Ch`). Function `F8h` is above that bound and is rejected.

const IBM: u16 = 0xffff;
const MAX_CALL: u8 = 0x24;
const MAX_COMMAND: u8 = 0x6c;

pub fn call5_ok(function: u8) -> bool {
    function <= MAX_CALL
}

pub fn command_ok(function: u8) -> bool {
    function <= MAX_COMMAND
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"MSDISP IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" MC");
    put_hex8(MAX_COMMAND);
    crate::bios::mscon::con_write_bytes(if call5_ok(0x24) { b" C24OK" } else { b" C24BAD" });
    crate::bios::mscon::con_write_bytes(if call5_ok(0x25) { b" C25OK" } else { b" C25BAD" });
    crate::bios::mscon::con_write_bytes(if command_ok(0xf8) { b" OEMF8" } else { b" OEMNO" });
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
