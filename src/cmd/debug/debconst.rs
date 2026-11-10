//! Debugger tables from `CMD/DEBUG/DEBCONST.ASM`.
//!
//! Eight-bit names run AL, CL, DL, BL, AH, CH, DH, BH. Sixteen-bit names run
//! AX, CX, DX, BX, then the pointer and index names. Segment overrides are
//! stored ES, CS, SS, DS. The assembler table has one opcode byte and one
//! handler for each mnemonic. Saved IP starts at 100h and the flags start
//! at F202h.

const REG8: &[u8] = b"ALCLDLBLAHCHDHBH";
const REG16: &[u8] = b"AXCXDXBXSPBPSIDI";
const SREG: &[u8] = b"ESCSSSDS";
const SIZES: &[u8] = b"BYWODWQWTB";
const SHIFTS: [&[u8]; 8] = [
    b"ROL", b"ROR", b"RCL", b"RCR", b"SHL", b"SHR", b"???", b"SAR",
];
const IMMEDIATE: [&[u8]; 8] = [
    b"ADD", b"OR", b"ADC", b"SBB", b"AND", b"SUB", b"XOR", b"CMP",
];
const GROUP1: [&[u8]; 8] = [
    b"TEST", b"???", b"NOT", b"NEG", b"MUL", b"IMUL", b"DIV", b"IDIV",
];
const GROUP2: [&[u8]; 8] = [
    b"INC", b"DEC", b"CALL", b"CALL", b"JMP", b"JMP", b"PUSH", b"???",
];
const MAX_OPCODES: u16 = 193;
const INITIAL_SP: u16 = 0x005A;
const INITIAL_IP: u16 = 0x0100;
const INITIAL_FLAGS: u16 = 0xF202;
const ROW_MASK: u8 = 0x0F;
const DUMP_BYTES: u16 = 128;
const REGISTERS_PER_LINE: u8 = 8;

fn pair(table: &[u8], index: usize) -> [u8; 2] {
    let start = index * 2;
    [table[start], table[start + 1]]
}

fn write_pair(label: &[u8], bytes: [u8; 2]) {
    crate::bios::mscon::con_write_bytes(label);
    put_hex8(bytes[0]);
    put_hex8(bytes[1]);
}

fn write_name(label: &[u8], name: &[u8]) {
    crate::bios::mscon::con_write_bytes(label);
    let mut index = 0usize;
    while index < name.len() {
        put_hex8(name[index]);
        index += 1;
    }
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DEBCONST");
    write_pair(b" R8", pair(REG8, 0));
    write_pair(b" R8", pair(REG8, 4));
    write_pair(b" R6", pair(REG16, 0));
    write_pair(b" R6", pair(REG16, 1));
    write_pair(b" R6", pair(REG16, 3));
    write_pair(b" SR", pair(SREG, 0));
    write_pair(b" SR", pair(SREG, 1));
    write_pair(b" SZ", pair(SIZES, 0));
    write_pair(b" SZ", pair(SIZES, 2));
    write_name(b" SH", SHIFTS[0]);
    write_name(b" SH", SHIFTS[7]);
    write_name(b" IM", IMMEDIATE[0]);
    write_name(b" IM", IMMEDIATE[7]);
    write_name(b" G1", GROUP1[0]);
    write_name(b" G1", GROUP1[1]);
    write_name(b" G2", GROUP2[0]);
    write_name(b" G2", GROUP2[7]);
    crate::bios::mscon::con_write_bytes(b" NP90 RTC3 ITCC INEC XLD7 HLF4 AA37 CLF8 STFB MX");
    put_hex16(MAX_OPCODES);
    crate::bios::mscon::con_write_bytes(b" SP");
    put_hex16(INITIAL_SP);
    crate::bios::mscon::con_write_bytes(b" IP");
    put_hex16(INITIAL_IP);
    crate::bios::mscon::con_write_bytes(b" FL");
    put_hex16(INITIAL_FLAGS);
    crate::bios::mscon::con_write_bytes(b" WZ");
    put_hex8(ROW_MASK);
    crate::bios::mscon::con_write_bytes(b" LN");
    put_hex16(DUMP_BYTES);
    crate::bios::mscon::con_write_bytes(b" NR");
    put_hex8(REGISTERS_PER_LINE);
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
