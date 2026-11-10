//! Equates from `CMD/COMP/COMPEQ.INC`.
//!
//! A code-page attribute is a system binary. The extended-open control word
//! keeps the code-page check in bit 8, the missing-file action in the next
//! four bits, and the existing-file action in the low four. The message
//! block is twelve bytes because the keyboard prompt adds an input pointer.
//! The substitution block is eleven bytes and pads with a space.

const BINARY: u16 = 0x02;
const SYSTEM: u16 = 0x8000;
const CONTROL: u16 = 0x0101;
const OPEN_MODE: u16 = 0x82;
const CLASS_UTILITY: u16 = 0xFF;
const MESSAGE_BYTES: u16 = 12;
const SUBST_BYTES: u16 = 11;

fn code_page_check(control: u16) -> u8 {
    ((control >> 8) & 1) as u8
}

fn missing_file(control: u16) -> u8 {
    ((control >> 4) & 0x0F) as u8
}

fn existing_file(control: u16) -> u8 {
    (control & 0x0F) as u8
}

fn no_inherit(mode: u16) -> u8 {
    ((mode >> 7) & 1) as u8
}

fn sharing(mode: u16) -> u8 {
    ((mode >> 4) & 0x07) as u8
}

fn access(mode: u16) -> u8 {
    (mode & 0x07) as u8
}

fn tag8(label: &[u8], value: u16) {
    crate::bios::mscon::con_write_bytes(label);
    put_hex8(value as u8);
}

fn tag16(label: &[u8], value: u16) {
    crate::bios::mscon::con_write_bytes(label);
    put_hex16(value);
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"COMPEQ");
    tag8(b" EA", BINARY);
    tag16(b" SY", SYSTEM);
    tag8(b" VR", 0x00);
    tag8(b" NU", 0x00);
    tag8(b" OP", 0x01);
    tag8(b" ZC", 0x00);
    tag8(b" FP", 0x01);
    tag8(b" TP", 0x02);
    tag8(b" YS", 0x01);
    tag8(b" KC", 0xC1);
    tag8(b" KI", 0xFF);
    tag8(b" LN", 127);
    tag8(b" KN", 0x01);
    tag8(b" DO", 0x02);
    tag8(b" IO", 0x06);
    tag8(b" BK", 0x0A);
    tag8(b" IS", 0x0B);
    tag8(b" CK", 0x0C);
    tag8(b" FO", 0x0F);
    tag8(b" FC", 0x10);
    tag8(b" SF", 0x11);
    tag8(b" SN", 0x12);
    tag8(b" SD", 0x1A);
    tag8(b" DD", 0x19);
    tag8(b" YN", 0x23);
    tag8(b" SV", 0x25);
    tag8(b" RB", 0x27);
    tag8(b" PF", 0x29);
    tag8(b" GV", 0x30);
    tag8(b" CD", 0x3B);
    tag8(b" HO", 0x3D);
    tag8(b" HC", 0x3E);
    tag8(b" WR", 0x40);
    tag8(b" CU", 0x47);
    tag8(b" RT", 0x4C);
    tag8(b" GC", 0x65);
    tag16(b" SO", 0x0001);
    tag16(b" SE", 0x0002);
    tag8(b" RO", 0x00);
    tag8(b" CC", 0x33);
    tag8(b" GS", 0x03);
    tag8(b" SS", 0x04);
    tag8(b" OF", 0x00);
    tag8(b" ON", 0x01);
    tag8(b" OB", 0x40);
    tag8(b" ZP", 0x00);
    tag8(b" NA", 0x00);
    tag8(b" OM", OPEN_MODE);
    tag16(b" FN", CONTROL);
    tag8(b" RD", 0x00);
    tag8(b" XO", 0x6C);
    tag8(b" AL", 0x57);
    tag8(b" GL", 0x02);
    tag8(b" EE", 0x59);
    tag8(b" OK", 0x00);
    tag8(b" CB", 0x01);
    tag8(b" VB", 0x01);
    tag8(b" NS", 0x00);
    tag8(b" S1", 0x01);
    tag8(b" S2", 0x02);
    tag8(b" S3", 0x03);
    tag8(b" C1", 0x01);
    tag8(b" C2", 0x02);
    tag8(b" CA", CLASS_UTILITY);
    tag16(b" CW", CLASS_UTILITY << 8);
    tag16(b" DS", MESSAGE_BYTES);
    tag16(b" SL", SUBST_BYTES);
    tag8(b" PD", b' ' as u16);
    tag8(b" CP", code_page_check(CONTROL) as u16);
    tag8(b" MF", missing_file(CONTROL) as u16);
    tag8(b" EF", existing_file(CONTROL) as u16);
    tag8(b" IH", no_inherit(OPEN_MODE) as u16);
    tag8(b" SH", sharing(OPEN_MODE) as u16);
    tag8(b" AC", access(OPEN_MODE) as u16);
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
