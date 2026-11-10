//! Process exit from `DOS/MSPROC.ASM`.
//!
//! The file includes `MSSW.ASM`, where `IBMVER` is true, so escape is 0 and
//! cancel is Escape. `$WAIT` returns the previous exit word and clears it.
//! `$EXIT` records Ctrl-C in the high byte. `$Keep_process` keeps at least
//! six paragraphs. `Stay_resident` rounds the offset up to paragraphs and
//! then uses that keep path. `$ABORT` stores a zero code and restores the
//! six saved interrupt words.

const IBM: u16 = 0xffff;
const ESCAPE: u8 = 0;
const CANCEL: u8 = 27;
const PRINTER_TOGGLE: u16 = 0xffff;
const KIND_TERMINATE: u8 = 0;
const KIND_CTRL_C: u8 = 1;
const KIND_KEEP: u8 = 3;
const MIN_PARAGRAPHS: u16 = 6;
const SAVED_VECTOR_WORDS: u8 = 6;

pub struct Process {
    pub segment: u16,
    pub exit_code: u16,
    pub kind: u8,
    pub ctrl_c: bool,
    pub block_end: u16,
    pub restored: u8,
}

pub fn wait(process: &mut Process) -> u16 {
    let code = process.exit_code;
    process.exit_code = 0;
    code
}

pub fn exit(process: &mut Process, code: u8) -> u16 {
    let interrupted = process.ctrl_c;
    process.ctrl_c = false;
    process.kind = if interrupted {
        KIND_CTRL_C
    } else {
        KIND_TERMINATE
    };
    finish(process, code)
}

pub fn keep(process: &mut Process, offset: u16, code: u8) -> u16 {
    let paragraphs = if offset < MIN_PARAGRAPHS {
        MIN_PARAGRAPHS
    } else {
        offset
    };
    process.block_end = process.segment.wrapping_add(paragraphs);
    process.kind = KIND_KEEP;
    finish(process, code)
}

pub fn stay(process: &mut Process, offset: u16) -> u16 {
    let paragraphs = offset.wrapping_add(15) >> 4;
    keep(process, paragraphs, 0)
}

pub fn abort(process: &mut Process) -> u16 {
    process.kind = KIND_TERMINATE;
    finish(process, 0)
}

fn finish(process: &mut Process, code: u8) -> u16 {
    process.exit_code = (u16::from(process.kind) << 8) | u16::from(code);
    process.restored = SAVED_VECTOR_WORDS;
    process.exit_code
}

pub fn show() {
    let mut process = Process {
        segment: 0x2000,
        exit_code: 0,
        kind: 0,
        ctrl_c: false,
        block_end: 0,
        restored: 0,
    };
    let first = wait(&mut process);
    process.ctrl_c = true;
    let exited = exit(&mut process, 0x22);
    let _kept = keep(&mut process, 10, 0);
    let kept_end = process.block_end;
    let _resident = stay(&mut process, 100);
    let stayed_end = process.block_end;
    let aborted = abort(&mut process);
    crate::bios::mscon::con_write_bytes(b"MSPROC IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" ESC");
    put_hex8(ESCAPE);
    crate::bios::mscon::con_write_bytes(b" CAN");
    put_hex8(CANCEL);
    crate::bios::mscon::con_write_bytes(b" PRN");
    put_hex16(PRINTER_TOGGLE);
    crate::bios::mscon::con_write_bytes(b" WT");
    put_hex16(first);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex16(exited);
    crate::bios::mscon::con_write_bytes(b" KP");
    put_hex16(kept_end);
    crate::bios::mscon::con_write_bytes(b" SR");
    put_hex16(stayed_end);
    crate::bios::mscon::con_write_bytes(b" AB");
    put_hex16(aborted);
    crate::bios::mscon::con_write_bytes(b" RS");
    put_hex8(process.restored);
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
