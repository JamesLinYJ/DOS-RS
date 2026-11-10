//! `FOR` workspace from `CMD/COMMAND/FORDATA.ASM`.
//!
//! The loop record holds one argument block, the command tail, the list
//! bounds, a 64-word buffer, a 64-word search record, and the control letter.

const ARG_ELEMENT: usize = 2 + 1 + 2 + 2 + 2 + 2;
const ARG_MAX: usize = 64;
const ARG_BUFFER: usize = 256;
const COMMAND_BUFFER: usize = 128;
const FOR_WORDS: usize = 64;

fn argument_block() -> usize {
    ARG_MAX * ARG_ELEMENT + 2 + 2 + ARG_BUFFER * 2 + COMMAND_BUFFER
}

fn for_record() -> usize {
    argument_block() + 1 + 2 + 2 + 2 + FOR_WORDS * 2 + FOR_WORDS * 2 + 1
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"FORDATA AR");
    put_hex16(argument_block() as u16);
    crate::bios::mscon::con_write_bytes(b" FR");
    put_hex16(for_record() as u16);
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
