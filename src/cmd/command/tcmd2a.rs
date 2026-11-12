//! Default prompt from `CMD/COMMAND/TCMD2A.ASM`.
//!
//! `print_prompt` looks for a `PROMPT=` value. This environment has none,
//! so it prints the default drive letter from INT 21h AH=19h and `>`
//! (`SYM` in `COMEQU.ASM`).

const SYM: u8 = b'>';

/// `VERSION` from this file. CRLF, the version line, CRLF.
///
/// INT 21h AH=30h returns the major version in AL and the minor in AH.
/// Message 1040 substitutes those two numbers.
pub fn version() {
    let [major, minor] = crate::dos::getset::VERSION.0.to_le_bytes();
    crate::bios::mscon::con_write_bytes(b"\r\n");
    crate::bios::mscon::con_write_bytes(super::tdata::VER_MESSAGE);
    crate::bios::mscon::write_number(major);
    crate::bios::mscon::con_write_bytes(b".");
    crate::bios::mscon::write_number(minor);
    crate::bios::mscon::con_write_bytes(b"\r\n\r\n");
}

/// The serial console records a clear-screen command in the boot transcript.
pub fn cls() {
    crate::bios::mscon::con_write_bytes(b"CLS\r\n");
}

/// One trip through `print_prompt` when no `PROMPT` string is set.
pub fn print_default_prompt() {
    let drive = default_drive();
    let letter = b'A'.wrapping_add(drive);
    crate::bios::mscon::con_write_bytes(&[letter, SYM]);
}

fn default_drive() -> u8 {
    0
}
