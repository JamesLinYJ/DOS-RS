//! Message services from `CMD/COMP/COMPSM.ASM`.
//!
//! The utility name is COMP. Loading checks the DOS version. A bad version
//! is message 1 in the utility class. A failure to load the texts is the same
//! number with class 0. Both use standard error, no replacements, and no
//! input. A missing message reports a length of zero. Time and date
//! substitution stay off.

const STDERR: u16 = 2;
const UTILITY_CLASS: u8 = 0xFF;
const NO_INPUT: u8 = 0;

struct LoadProblem {
    number: u16,
    class: u8,
    handle: u16,
    replacements: u16,
    input: u8,
}

fn load_problem(bad_version: bool) -> LoadProblem {
    LoadProblem {
        number: 1,
        class: if bad_version { UTILITY_CLASS } else { 0 },
        handle: STDERR,
        replacements: 0,
        input: NO_INPUT,
    }
}

fn message_length(found: bool, size: u16) -> u16 {
    if found { size } else { 0 }
}

fn write_problem(problem: &LoadProblem) {
    crate::bios::mscon::con_write_bytes(b" N");
    put_hex8(problem.number as u8);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(problem.class);
    crate::bios::mscon::con_write_bytes(b" H");
    put_hex16(problem.handle);
    crate::bios::mscon::con_write_bytes(b" RP");
    put_hex16(problem.replacements);
    crate::bios::mscon::con_write_bytes(b" NI");
    put_hex8(problem.input);
}

pub fn show() {
    let version = load_problem(true);
    let missing_text = load_problem(false);
    crate::bios::mscon::con_write_bytes(b"COMPSM ID434F4D50 CR");
    let notice = b"The DOS COMP Utility";
    let mut index = 0usize;
    while index < notice.len() {
        put_hex8(notice[index]);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b" VR01 NR01 IN01 NU01 TM00 DT00 CLA CL1 CL2");
    write_problem(&version);
    write_problem(&missing_text);
    crate::bios::mscon::con_write_bytes(b" NF");
    put_hex16(message_length(false, 16));
    crate::bios::mscon::con_write_bytes(b" SZ");
    put_hex16(message_length(true, 16));
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
