//! `COPY` argument scan from `CMD/COMMAND/COPY.ASM`.
//!
//! Switches are checked before the argument count. One file name copies onto
//! the default drive. Two names use the second as the destination. A lone
//! trailing `+` is an overwrite. Concatenation turns the copy into ASCII
//! unless that source named `/B`. A device always stops at Ctrl-Z.

const SWITCH_V: u16 = 0x10;
const SWITCH_B: u16 = 0x08;
const SWITCH_A: u16 = 0x04;
const SWITCH_P: u16 = 0x02;
const F_SWITCH: u16 = 0x8000;
const F_BAD: u16 = 0x4000;
const SWITCH_COPY: u16 = SWITCH_V + SWITCH_A + SWITCH_B + F_SWITCH;
const MISSING: u8 = 2;
const TOO_MANY: u8 = 1;
const BAD_SWITCH: u8 = 3;
const PLUS_TAIL: u8 = 4;
const TRAILING: u8 = 5;

#[derive(Clone, Copy)]
struct Token {
    plus: bool,
    is_switch: bool,
    bits: u16,
    end: bool,
    text: &'static [u8],
}

struct Plan {
    error: u8,
    dest: u8,
    concat: u8,
    ascii: u8,
    verify: u8,
}

fn classify(tokens: &[Token]) -> Plan {
    let mut plus = 0u8;
    let mut args = 0u8;
    let mut objects = 0u8;
    let mut every_switch = 0u16;
    let mut destination = b"".as_slice();
    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        index += 1;
        objects = objects.wrapping_add(1);
        if token.plus {
            plus = 1;
        }
        if token.is_switch {
            let mut bits = token.bits;
            if bits & SWITCH_V != 0 && every_switch & SWITCH_V != 0 {
                bits |= F_BAD;
            }
            every_switch |= bits;
            if bits & !SWITCH_COPY != 0 {
                return Plan { error: BAD_SWITCH, dest: 0, concat: 0, ascii: 0, verify: 0 };
            }
            if token.end {
                break;
            }
            continue;
        }
        if token.end {
            break;
        }
        if !token.plus {
            args = args.wrapping_add(1);
        }
        destination = token.text;
    }
    if plus == 1 && args == 1 && objects == 2 {
        return Plan { error: PLUS_TAIL, dest: 0, concat: 0, ascii: 0, verify: 0 };
    }
    let concat = plus;
    if args == 0 {
        return Plan { error: MISSING, dest: 0, concat: 0, ascii: 0, verify: 0 };
    }
    if args > 2 {
        return Plan { error: TOO_MANY, dest: 0, concat: 0, ascii: 0, verify: 0 };
    }
    let dest = if args == 1 {
        1
    } else if destination.len() == 2 && destination[1] == b':' {
        2
    } else if destination.last() == Some(&b'\\')
        && destination.len() >= 2
        && destination[destination.len() - 2] == b':'
    {
        3
    } else if destination.last() == Some(&b'\\') {
        return Plan { error: TRAILING, dest: 0, concat: 0, ascii: 0, verify: 0 };
    } else {
        0
    };
    let mut ascii = 0u8;
    if source_bits(tokens) & SWITCH_B == 0 && concat != 0 {
        ascii = SWITCH_A as u8;
    }
    Plan {
        error: 0,
        dest,
        concat,
        ascii,
        verify: u8::from(every_switch & SWITCH_V != 0),
    }
}

fn source_bits(tokens: &[Token]) -> u16 {
    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        if !token.is_switch {
            return token.bits;
        }
        index += 1;
    }
    0
}

fn drive_number(name: &[u8]) -> u8 {
    let letter = if name.len() >= 2 && name[1] == b':' { name[0] } else { b'@' };
    (letter | 0x20).wrapping_sub(0x60)
}

fn kept_bytes(data: &[u8], device: bool, ascii: bool) -> (u8, u8) {
    if !device && !ascii {
        return (data.len() as u8, 0);
    }
    let mut index = 0usize;
    let mut remaining = data.len();
    while remaining != 0 {
        let byte = data[index];
        index += 1;
        remaining -= 1;
        if byte == 0x1A {
            return ((data.len() - remaining - 1) as u8, 1);
        }
    }
    (data.len() as u8, 0)
}

fn write_plan(label: &[u8], plan: Plan) {
    crate::bios::mscon::con_write_bytes(label);
    put_hex8(plan.error);
    put_hex8(plan.dest);
    put_hex8(plan.concat);
    put_hex8(plan.ascii);
    put_hex8(plan.verify);
}

pub fn show() {
    let end = Token { plus: false, is_switch: false, bits: 0, end: true, text: b"" };
    let file = Token { plus: false, is_switch: false, bits: 0, end: false, text: b"FOO" };
    let other = Token { plus: false, is_switch: false, bits: 0, end: false, text: b"BAR" };
    let third = Token { plus: false, is_switch: false, bits: 0, end: false, text: b"BAZ" };
    let plus_end = Token { plus: true, is_switch: false, bits: 0, end: true, text: b"" };
    let plus = Token { plus: true, is_switch: false, bits: 0, end: false, text: b"B" };
    let binary = Token { plus: false, is_switch: false, bits: SWITCH_B, end: false, text: b"A" };
    let verify = Token {
        plus: false,
        is_switch: true,
        bits: F_SWITCH | SWITCH_V,
        end: false,
        text: b"/V",
    };
    let verify_again = Token {
        plus: false,
        is_switch: true,
        bits: F_SWITCH | SWITCH_V,
        end: true,
        text: b"/V",
    };
    let pause = Token {
        plus: false,
        is_switch: true,
        bits: F_SWITCH | SWITCH_P,
        end: true,
        text: b"/P",
    };
    let drive = Token { plus: false, is_switch: false, bits: 0, end: false, text: b"A:" };
    let root = Token { plus: false, is_switch: false, bits: 0, end: false, text: b"A:\\" };
    let slash = Token { plus: false, is_switch: false, bits: 0, end: false, text: b"FOO\\" };
    let empty = Token { plus: false, is_switch: false, bits: 0, end: true, text: b"" };
    let sample = b"AB\x1aCD";

    crate::bios::mscon::con_write_bytes(b"COPY ");
    write_plan(b"MS", classify(&[empty]));
    write_plan(b" ON", classify(&[file, end]));
    write_plan(b" TW", classify(&[file, other, end]));
    write_plan(b" TM", classify(&[file, other, third, end]));
    write_plan(b" OW", classify(&[file, plus_end]));
    write_plan(b" CD", classify(&[file, plus, end]));
    write_plan(b" CC", classify(&[file, plus, other, end]));
    write_plan(b" BB", classify(&[binary, plus, end]));
    write_plan(b" VV", classify(&[verify, file, end]));
    write_plan(b" BD", classify(&[verify, verify_again]));
    write_plan(b" BP", classify(&[pause]));
    write_plan(b" DR", classify(&[file, drive, end]));
    write_plan(b" SL", classify(&[file, root, end]));
    write_plan(b" SE", classify(&[file, slash, end]));
    crate::bios::mscon::con_write_bytes(b" DA");
    put_hex8(drive_number(b"A:FOO"));
    crate::bios::mscon::con_write_bytes(b" DC");
    put_hex8(drive_number(b"c:X"));
    crate::bios::mscon::con_write_bytes(b" DN");
    put_hex8(drive_number(b"FOO"));
    let (binary_len, binary_eof) = kept_bytes(sample, false, false);
    crate::bios::mscon::con_write_bytes(b" RB");
    put_hex8(binary_len);
    put_hex8(binary_eof);
    let (ascii_len, ascii_eof) = kept_bytes(sample, false, true);
    crate::bios::mscon::con_write_bytes(b" RA");
    put_hex8(ascii_len);
    put_hex8(ascii_eof);
    let (device_len, device_eof) = kept_bytes(sample, true, false);
    crate::bios::mscon::con_write_bytes(b" RZ");
    put_hex8(device_len);
    put_hex8(device_eof);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
