//! Process exit from `DOS/PROC.ASM`.
//!
//! `$WAIT` returns the previous exit word and clears it. `$EXIT` records a
//! normal end, then replaces that with Ctrl-C when `DidCTRLC` was set.
//! `$Keep_process` keeps at least six paragraphs and skips the block update
//! when `SetBlock` fails. `Stay_resident` rounds the byte offset up with a
//! rotate through carry. `$ABORT` stores type 0 and copies six interrupt words
//! from PDB offset 10 onto the INT 22h vector.

const KIND_TERMINATE: u8 = 0;
const KIND_CTRL_C: u8 = 1;
const KIND_ABORT: u8 = 0;
const KIND_KEEP: u8 = 3;
const MIN_PARAGRAPHS: u16 = 6;
const SAVED_AT: u16 = 10;
const SAVED_WORDS: u8 = 6;
const VECTOR_AT: u16 = 0x88;

pub struct Process {
    pub segment: u16,
    pub exit_code: u16,
    pub kind: u8,
    pub ctrl_c: bool,
    pub block_end: u16,
    pub restored: u8,
    pub vector_at: u16,
}

/// `$WAIT`.
pub fn wait(process: &mut Process) -> u16 {
    let code = process.exit_code;
    process.exit_code = 0;
    code
}

/// `$EXIT`. Terminate is stored first. A pending Ctrl-C replaces it.
pub fn exit(process: &mut Process, code: u8) -> u16 {
    let interrupted = process.ctrl_c;
    process.ctrl_c = false;
    process.kind = KIND_TERMINATE;
    if interrupted {
        process.kind = KIND_CTRL_C;
    }
    finish(process, code)
}

/// `$Keep_process`.
pub fn keep(process: &mut Process, offset: u16, code: u8, resized: bool) -> u16 {
    process.kind = KIND_KEEP;
    let paragraphs = if offset < MIN_PARAGRAPHS {
        MIN_PARAGRAPHS
    } else {
        offset
    };
    if resized {
        process.block_end = process.segment.wrapping_add(paragraphs);
    }
    finish(process, code)
}

/// `Stay_resident`. `ADD` then `RCR` then three more shifts.
pub fn stay(process: &mut Process, offset: u16, resized: bool) -> u16 {
    keep(process, round_paragraphs(offset), 0, resized)
}

/// `$ABORT`. The exit type and the zero code share the value 0.
pub fn abort(process: &mut Process) -> u16 {
    process.kind = KIND_ABORT;
    finish(process, 0)
}

pub fn round_paragraphs(offset: u16) -> u16 {
    let (sum, carry) = offset.overflowing_add(15);
    let rotated = (sum >> 1) | if carry { 0x8000 } else { 0 };
    rotated >> 3
}

pub fn show() {
    let mut waiting = fresh();
    waiting.exit_code = 0x00ab;
    let previous = wait(&mut waiting);
    let cleared = waiting.exit_code;
    let mut ended = fresh();
    let exited = exit(&mut ended, 0x22);
    let mut broken = fresh();
    broken.ctrl_c = true;
    let ctrl_c = exit(&mut broken, 0x22);
    let mut kept = fresh();
    let _ = keep(&mut kept, 5, 0, true);
    let mut above = fresh();
    let _ = keep(&mut above, 7, 0, true);
    let mut failed = fresh();
    let _ = keep(&mut failed, 5, 0, false);
    let mut resident = fresh();
    let _ = stay(&mut resident, 0xfff1, true);
    let mut aborted = fresh();
    let status = abort(&mut aborted);
    crate::bios::mscon::con_write_bytes(b"PROC WT");
    put_hex16(previous);
    crate::bios::mscon::con_write_bytes(b" Z");
    put_hex16(cleared);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex16(exited);
    crate::bios::mscon::con_write_bytes(b" CC");
    put_hex16(ctrl_c);
    crate::bios::mscon::con_write_bytes(b" KP");
    put_hex16(kept.block_end);
    crate::bios::mscon::con_write_bytes(b" K7");
    put_hex16(above.block_end);
    crate::bios::mscon::con_write_bytes(b" KF");
    put_hex16(failed.block_end);
    crate::bios::mscon::con_write_bytes(b" OV");
    put_hex16(resident.block_end);
    crate::bios::mscon::con_write_bytes(b" AB");
    put_hex16(status);
    crate::bios::mscon::con_write_bytes(b" RS");
    put_hex8(aborted.restored);
    crate::bios::mscon::con_write_bytes(b" AT");
    put_hex16(aborted.vector_at);
    crate::bios::mscon::con_write_bytes(b" SV");
    put_hex16(SAVED_AT);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn finish(process: &mut Process, code: u8) -> u16 {
    process.exit_code = (u16::from(process.kind) << 8) | u16::from(code);
    process.restored = SAVED_WORDS;
    process.vector_at = VECTOR_AT;
    process.exit_code
}

fn fresh() -> Process {
    Process {
        segment: 0x2000,
        exit_code: 0,
        kind: 0,
        ctrl_c: false,
        block_end: 0,
        restored: 0,
        vector_at: 0,
    }
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
