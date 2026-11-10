//! Message printing from `CMD/COMMAND/TPRINTF.ASM`.
//!
//! Standard output is handle 1 and standard error is handle 2. A date or
//! time substitution is copied, then the original value is cleared. A zero
//! segment is filled unless the substitution is a date or time. The first
//! substitution is redirected when it names the shared string slot. The
//! class and continuation flag are reset before the message is shown. A
//! failure on standard error leaves the command loop. A piped failure turns
//! the pipe off. Any other failure becomes an extended error.

const UTILITY: u8 = 0xFF;
const EXTENDED: u8 = 1;
const DATE_OR_TIME: u8 = 0x04;
const TRANSIENT: u16 = 0x0F00;
const BLOCK: usize = 11;

struct Piece {
    value: u32,
    flag: u8,
}

struct Request<'a> {
    error_output: bool,
    class: u8,
    pieces: &'a [Piece],
    slot: u16,
    pointed: u16,
    failure: Option<u16>,
    pipe: bool,
}

struct Prepared {
    handle: u8,
    class: u8,
    continuation: u8,
    copied: u32,
    source: u32,
    redirected: bool,
    extended_cleared: bool,
    outcome: u8,
    error: u16,
}

fn prepare(request: &Request<'_>) -> Prepared {
    let mut copied = 0u32;
    let mut source = 0u32;
    let mut redirected = false;
    let mut extended_cleared = false;
    if !request.pieces.is_empty() {
        let first = &request.pieces[0];
        copied = first.value;
        source = first.value;
        if first.flag & DATE_OR_TIME != 0 {
            source = 0;
        }
        if request.class != UTILITY {
            extended_cleared = true;
        }
        let segment = (copied >> 16) as u16;
        if segment == 0 && first.flag & DATE_OR_TIME == 0 {
            copied = (u32::from(TRANSIENT) << 16) | (copied & 0xFFFF);
        }
        if copied as u16 == request.slot {
            copied = (copied & 0xFFFF_0000) | u32::from(request.pointed);
            redirected = true;
        }
    }
    let outcome = match request.failure {
        None => 0,
        Some(_) if request.error_output => 1,
        Some(_) if request.pipe => 2,
        Some(_) => 3,
    };
    Prepared {
        handle: if request.error_output { 2 } else { 1 },
        class: UTILITY,
        continuation: 0,
        copied,
        source,
        redirected,
        extended_cleared,
        outcome,
        error: request.failure.unwrap_or(0),
    }
}

fn write_prepared(prepared: &Prepared, counted: bool) {
    crate::bios::mscon::con_write_bytes(b" HD");
    put_hex8(prepared.handle);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(prepared.class);
    crate::bios::mscon::con_write_bytes(b" CT");
    put_hex8(prepared.continuation);
    if counted {
        crate::bios::mscon::con_write_bytes(b" CP");
        put_hex32(prepared.copied);
        crate::bios::mscon::con_write_bytes(b" SR");
        put_hex32(prepared.source);
        crate::bios::mscon::con_write_bytes(b" ID");
        put_hex8(u8::from(prepared.redirected));
    } else {
        crate::bios::mscon::con_write_bytes(b" NS");
    }
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(u8::from(prepared.extended_cleared));
    crate::bios::mscon::con_write_bytes(b" ER");
    put_hex8(prepared.outcome);
    if prepared.outcome == 3 {
        crate::bios::mscon::con_write_bytes(b" CD");
        put_hex16(prepared.error);
    }
}

pub fn show() {
    let text = [Piece { value: 0x0000_0010, flag: 0 }];
    let dated = [Piece { value: 0x1111_2222, flag: DATE_OR_TIME }];
    let shared = [Piece { value: 0x0000_0200, flag: 0 }];
    crate::bios::mscon::con_write_bytes(b"TPRINTF");
    write_prepared(&prepare(&Request { error_output: false, class: UTILITY, pieces: &text, slot: 0x0200, pointed: 0x0300, failure: None, pipe: false }), true);
    write_prepared(&prepare(&Request { error_output: true, class: UTILITY, pieces: &dated, slot: 0x0200, pointed: 0x0300, failure: None, pipe: false }), true);
    write_prepared(&prepare(&Request { error_output: false, class: EXTENDED, pieces: &shared, slot: 0x0200, pointed: 0x0300, failure: None, pipe: false }), true);
    write_prepared(&prepare(&Request { error_output: false, class: EXTENDED, pieces: &[], slot: 0, pointed: 0, failure: None, pipe: false }), false);
    write_prepared(&prepare(&Request { error_output: false, class: UTILITY, pieces: &[], slot: 0, pointed: 0, failure: Some(5), pipe: false }), false);
    write_prepared(&prepare(&Request { error_output: true, class: UTILITY, pieces: &[], slot: 0, pointed: 0, failure: Some(5), pipe: false }), false);
    write_prepared(&prepare(&Request { error_output: false, class: UTILITY, pieces: &[], slot: 0, pointed: 0, failure: Some(5), pipe: true }), false);
    crate::bios::mscon::con_write_bytes(b" SZ");
    put_hex8(BLOCK as u8);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex32(value: u32) {
    put_hex16((value >> 16) as u16);
    put_hex16(value as u16);
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
