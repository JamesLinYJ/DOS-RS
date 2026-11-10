//! Standard build switches from `DOS/STDSW.ASM`.
//!
//! `IBM` and `WANG` are off, so escape is `1Bh` and cancel is Ctrl-X.
//! `ALTVECT` is off, so terminate stays at interrupt `22h`. Rainbow and
//! high memory are off. An IBM build would use a null escape and Escape
//! as cancel. A Wang build would use `1Fh`.

const TRUE: u16 = 0xffff;
const FALSE: u16 = 0;

pub const MICROSOFT: u16 = TRUE;
pub const IBM: u16 = FALSE;
pub const WANG: u16 = FALSE;
pub const ALTERNATE_VECTOR: u16 = FALSE;
pub const RAINBOW: u16 = FALSE;
pub const HIGH_MEMORY: u16 = FALSE;
pub const ZERO_EXTEND: u16 = TRUE;

/// Escape. IBM uses 0, Wang uses `1Fh`, and the standard build uses `1Bh`.
pub fn escape(ibm: bool, wang: bool) -> u8 {
    if ibm {
        0
    } else if wang {
        0x1f
    } else {
        0x1b
    }
}

/// Cancel. IBM uses Escape. The standard build uses Ctrl-X.
pub fn cancel(ibm: bool) -> u8 {
    if ibm { 27 } else { b'X' - b'@' }
}

/// Printer echo. IBM toggles with one key.
pub fn printer_toggle(ibm: bool) -> u16 {
    if ibm { TRUE } else { FALSE }
}

/// Terminate interrupt. Alternate vectors use `F0h`.
pub fn terminate_interrupt(alternate: bool) -> u8 {
    if alternate { 0xf0 } else { 0x22 }
}

pub fn show() {
    let chosen_escape = escape(IBM != 0, WANG != 0);
    let chosen_cancel = cancel(IBM != 0);
    let printer = printer_toggle(IBM != 0);
    let terminate = terminate_interrupt(ALTERNATE_VECTOR != 0);
    let ibm_escape = escape(true, false);
    let ibm_cancel = cancel(true);
    let wang_escape = escape(false, true);
    crate::bios::mscon::con_write_bytes(b"STDSW MS");
    put_hex16(MICROSOFT);
    crate::bios::mscon::con_write_bytes(b" IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" WG");
    put_hex16(WANG);
    crate::bios::mscon::con_write_bytes(b" AV");
    put_hex16(ALTERNATE_VECTOR);
    crate::bios::mscon::con_write_bytes(b" RB");
    put_hex16(RAINBOW);
    crate::bios::mscon::con_write_bytes(b" HM");
    put_hex16(HIGH_MEMORY);
    crate::bios::mscon::con_write_bytes(b" ESC");
    put_hex8(chosen_escape);
    crate::bios::mscon::con_write_bytes(b" CAN");
    put_hex8(chosen_cancel);
    crate::bios::mscon::con_write_bytes(b" PRN");
    put_hex16(printer);
    crate::bios::mscon::con_write_bytes(b" EXT");
    put_hex16(ZERO_EXTEND);
    crate::bios::mscon::con_write_bytes(b" INT");
    put_hex8(terminate);
    crate::bios::mscon::con_write_bytes(b" IESC");
    put_hex8(ibm_escape);
    crate::bios::mscon::con_write_bytes(b" ICAN");
    put_hex8(ibm_cancel);
    crate::bios::mscon::con_write_bytes(b" WESC");
    put_hex8(wang_escape);
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
