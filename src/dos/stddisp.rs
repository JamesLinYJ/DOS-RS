//! Standard system-call dispatch from `DOS/STDDISP.ASM`.
//!
//! `IBM` is false, so function `F8h` installs an OEM handler and `F9h`
//! through `FFh` jump to it. Until that handler is installed it is `-1`,
//! and those calls fail. `CALL 5` still stops at `24h`.

const IBM: u16 = 0;
const MAX_CALL: u8 = 0x24;
const MAX_COMMAND: u8 = 0x6c;
const SET_OEM: u8 = 0xf8;

pub fn call5_ok(function: u8) -> bool {
    function <= MAX_CALL
}

/// `Ok(true)` accepts the call. `Ok(false)` installs the OEM handler.
/// `Err` is `BadCall`.
pub fn command(function: u8, handler_set: bool) -> Result<bool, ()> {
    if function >= SET_OEM {
        if function == SET_OEM {
            return Ok(false);
        }
        if !handler_set {
            return Err(());
        }
        return Ok(true);
    }
    if function <= MAX_COMMAND {
        Ok(true)
    } else {
        Err(())
    }
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"STDDISP IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(if call5_ok(0x24) { b" C24OK" } else { b" C24BAD" });
    crate::bios::mscon::con_write_bytes(if call5_ok(0x25) { b" C25OK" } else { b" C25BAD" });
    crate::bios::mscon::con_write_bytes(match command(SET_OEM, false) {
        Ok(false) => b" SETF8",
        _ => b" BADF8",
    });
    crate::bios::mscon::con_write_bytes(match command(0xf9, false) {
        Err(()) => b" BADF9",
        _ => b" OKF9",
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
