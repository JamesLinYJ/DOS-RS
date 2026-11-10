//! Argument array from `CMD/COMMAND/PARSE2.ASM`.
//!
//! A leading switch is kept as its own argument and is not attached backward.
//! A switch after a real argument is recorded on that argument and on the
//! line. Sixty-four arguments fit. The sixty-fifth is too many. A token that
//! reaches the last buffer byte overflows. Stars are not expanded here.

const ARG_LIMIT: u8 = 64;
const BUFFER_LIMIT: usize = 256;
const TOO_MANY: u8 = 1;
const OVERFLOW: u8 = 2;
const SWITCH_FLAG: u8 = 0x01;
const ARG_BYTES: u16 = 11;

#[derive(Clone, Copy)]
struct Token {
    flags: u8,
    switches: u16,
    len: usize,
    carriage: bool,
}

struct Parsed {
    count: u8,
    attached: u16,
    line_switches: u16,
    copied: u8,
    copy_flag: u8,
    error: u8,
}

fn parse_line(for_active: bool, tokens: &[Token]) -> Parsed {
    let mut parsed = Parsed {
        count: 0,
        attached: 0,
        line_switches: 0,
        copied: u8::from(!for_active),
        copy_flag: 0,
        error: 0,
    };
    let mut last_real: i16 = -1;
    let mut used = 0usize;
    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        index += 1;
        if token.carriage {
            if token.switches != 0 {
                push(&mut parsed, &mut last_real, &mut used, token);
            }
            break;
        }
        parsed.copy_flag = 2;
        if !push(&mut parsed, &mut last_real, &mut used, token) {
            break;
        }
    }
    parsed
}

fn push(parsed: &mut Parsed, last_real: &mut i16, used: &mut usize, token: &Token) -> bool {
    if token.flags & SWITCH_FLAG != 0 {
        if *last_real != -1 {
            parsed.attached |= token.switches;
            parsed.line_switches |= token.switches;
        }
    } else {
        *last_real = i16::from(parsed.count);
    }
    if parsed.count >= ARG_LIMIT {
        parsed.error = TOO_MANY;
        return false;
    }
    parsed.count += 1;
    if *used + token.len >= BUFFER_LIMIT - 1 {
        parsed.error = OVERFLOW;
        return false;
    }
    *used += token.len + 1;
    true
}

fn argv_offset(index: u16) -> u16 {
    index * ARG_BYTES
}

pub fn show() {
    let real = Token { flags: 0, switches: 0, len: 1, carriage: false };
    let switched = Token { flags: SWITCH_FLAG, switches: 0x10, len: 1, carriage: false };
    let end = Token { flags: 0, switches: 0, len: 0, carriage: true };
    let trailing = Token { flags: SWITCH_FLAG, switches: 0x10, len: 1, carriage: true };
    let huge = Token { flags: 0, switches: 0, len: 255, carriage: false };
    let many = [real; 65];
    let plain = parse_line(false, &[real, end]);
    let inside_for = parse_line(true, &[real, end]);
    let after = parse_line(false, &[real, switched, end]);
    let leading = parse_line(false, &[switched, real, end]);
    let too_many = parse_line(false, &many);
    let overflow = parse_line(false, &[huge, end]);
    let bare = parse_line(false, &[end]);
    let hung = parse_line(false, &[trailing]);

    crate::bios::mscon::con_write_bytes(b"PARSE2 ");
    write(&plain);
    write(&inside_for);
    write(&after);
    write(&leading);
    write(&too_many);
    write(&overflow);
    write(&bare);
    write(&hung);
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex16(argv_offset(2));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write(parsed: &Parsed) {
    put_hex8(parsed.copied);
    put_hex8(parsed.copy_flag);
    put_hex8(parsed.count);
    put_hex16(parsed.attached);
    put_hex16(parsed.line_switches);
    put_hex8(parsed.error);
    crate::bios::mscon::con_write_bytes(b" ");
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
