//! Standard process build from `DOS/STDPROC.ASM`.
//!
//! `STDSW.ASM` turns `IBM` off, so escape is `1Bh` and cancel is Ctrl-X.
//! The included process calls are the same ones the IBM wrapper uses.

const IBM: u16 = 0;
const ESCAPE: u8 = 0x1b;
const CANCEL: u8 = b'X' - b'@';
const PRINTER_TOGGLE: u16 = 0;
const HIGH_MEMORY: u16 = 0;

pub fn show() {
    let mut process = crate::dos::msproc::Process {
        segment: 0x2000,
        exit_code: 0,
        kind: 0,
        ctrl_c: false,
        block_end: 0,
        restored: 0,
    };
    let first = crate::dos::msproc::wait(&mut process);
    crate::bios::mscon::con_write_bytes(b"STDPROC IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" ESC");
    put_hex8(ESCAPE);
    crate::bios::mscon::con_write_bytes(b" CAN");
    put_hex8(CANCEL);
    crate::bios::mscon::con_write_bytes(b" PRN");
    put_hex16(PRINTER_TOGGLE);
    crate::bios::mscon::con_write_bytes(b" HM");
    put_hex16(HIGH_MEMORY);
    crate::bios::mscon::con_write_bytes(b" WT");
    put_hex16(first);
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
