//! The line assembler from `CMD/DEBUG/DEBASM.ASM`.
//!
//! A blank line leaves assemble mode. An opcode is three bytes in the table,
//! `count * 2 + count`. `INT 3` stays one byte. `IN` and `OUT` accept `AL` or
//! `AX`, and a numeric port clears bit 3. Mode, register, and r/m pack into
//! one byte with shifts 6, 3, and 0.

const TABLE_STRIDE: usize = 3;

fn opcode_slot(names: &[&[u8]], wanted: &[u8]) -> Option<u16> {
    let mut index = 0usize;
    while index < names.len() {
        if names[index] == wanted {
            return Some((index * TABLE_STRIDE) as u16);
        }
        index += 1;
    }
    None
}

fn interrupt_short(value: u8) -> bool {
    value == 3
}

struct Port {
    wide: bool,
    delta: u8,
    from_register: bool,
    cleared: bool,
    immediate: u8,
    accepted: bool,
}

fn input_port(register: &[u8], from_register: bool, immediate: u8) -> Port {
    let wide = match register {
        b"AL" => false,
        b"AX" => true,
        _ => {
            return Port {
                wide: false,
                delta: 0,
                from_register,
                cleared: false,
                immediate,
                accepted: false,
            };
        }
    };
    Port {
        wide,
        delta: u8::from(wide),
        from_register,
        cleared: !from_register,
        immediate,
        accepted: true,
    }
}

fn modrm(mode: u8, register: u8, rm: u8) -> u8 {
    ((mode & 0x03) << 6) | ((register & 0x07) << 3) | (rm & 0x07)
}

fn write_port(port: &Port) {
    if !port.accepted {
        crate::bios::mscon::con_write_bytes(b" ER01");
        return;
    }
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(if port.wide { 0x10 } else { 0x08 });
    crate::bios::mscon::con_write_bytes(b" DL");
    put_hex8(port.delta);
    crate::bios::mscon::con_write_bytes(b" DX");
    put_hex8(u8::from(port.from_register));
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(u8::from(port.cleared));
    if !port.from_register {
        crate::bios::mscon::con_write_bytes(b" IM");
        put_hex8(port.immediate);
    }
}

pub fn show() {
    let names: &[&[u8]] = &[b"MOV", b"INT", b"IN", b"OUT"];
    crate::bios::mscon::con_write_bytes(b"DEBASM EM01 IX");
    put_hex16(opcode_slot(names, b"INT").unwrap_or(0xFFFF));
    crate::bios::mscon::con_write_bytes(b" NF");
    put_hex16(opcode_slot(names, b"ZZZ").unwrap_or(0xFFFF));
    crate::bios::mscon::con_write_bytes(b" SH");
    put_hex8(u8::from(interrupt_short(3)));
    crate::bios::mscon::con_write_bytes(b" SH");
    put_hex8(u8::from(interrupt_short(5)));
    write_port(&input_port(b"AL", true, 0));
    write_port(&input_port(b"AX", true, 0));
    write_port(&input_port(b"AL", false, 0x21));
    write_port(&input_port(b"AX", false, 0x21));
    write_port(&input_port(b"BL", true, 0));
    crate::bios::mscon::con_write_bytes(b" MR");
    put_hex8(modrm(0, 1, 5));
    crate::bios::mscon::con_write_bytes(b" MR");
    put_hex8(modrm(3, 7, 0));
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
