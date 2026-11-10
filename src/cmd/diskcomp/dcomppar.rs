//! Command-line parser from `CMD/DISKCOMP/DCOMPPAR.ASM`.
//!
//! Two optional drives are allowed, and one switch with the synonyms /1
//! and /8. End of line is checked before a nonzero code is treated as an
//! error. The first drive is stored at ordinal one and the second at
//! ordinal two. A switch whose second character is '1' selects one side;
//! any other switch selects eight sectors. A parse error sets the exit
//! code and returns the help message.

const FINE: u16 = 0xFFFF;
const NO_OPTION: u8 = 0xFF;
const HELP: u16 = 4;
const EXIT_PARMS: u8 = 1;

enum Token {
    End,
    Fault { code: u16, moved: bool },
    Drive(u8),
    Mark(u8),
}

struct Parsed {
    source: u8,
    target: u8,
    side: u8,
    sectors: u8,
    status: u16,
    exit_code: u8,
    message: u16,
    substitutions: u16,
}

fn parse(tokens: &[Token]) -> Parsed {
    let mut source = 0u8;
    let mut target = 0u8;
    let mut side = NO_OPTION;
    let mut sectors = NO_OPTION;
    let mut ordinal = 0u16;
    let mut index = 0usize;
    loop {
        let token = if index < tokens.len() { &tokens[index] } else { &Token::End };
        index += 1;
        match token {
            Token::End => {
                return Parsed {
                    source,
                    target,
                    side,
                    sectors,
                    status: FINE,
                    exit_code: 0,
                    message: 0,
                    substitutions: 0,
                };
            }
            Token::Fault { code, moved } => {
                return Parsed {
                    source,
                    target,
                    side,
                    sectors,
                    status: HELP,
                    exit_code: EXIT_PARMS,
                    message: *code,
                    substitutions: if *moved { 1 } else { 0 },
                };
            }
            Token::Drive(number) => {
                ordinal = ordinal.wrapping_add(1);
                if ordinal == 1 {
                    source = *number;
                } else {
                    target = *number;
                }
            }
            Token::Mark(mark) => {
                if *mark == b'1' {
                    side = 1;
                } else {
                    sectors = 1;
                }
            }
        }
    }
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DCOMPPAR MN00 MX02 SW01 KW00 FL0101 FN0000 TY03 TGFF CP0081 OR0000 NS02");
    write_parsed(&parse(&[Token::End]));
    write_parsed(&parse(&[Token::Drive(1), Token::Drive(2), Token::End]));
    write_parsed(&parse(&[Token::Mark(b'1'), Token::End]));
    write_parsed(&parse(&[Token::Mark(b'8'), Token::End]));
    write_parsed(&parse(&[Token::Fault { code: 9, moved: true }]));
    write_parsed(&parse(&[Token::Fault { code: 3, moved: false }]));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write_parsed(parsed: &Parsed) {
    crate::bios::mscon::con_write_bytes(b" S");
    put_hex8(parsed.source);
    crate::bios::mscon::con_write_bytes(b" T");
    put_hex8(parsed.target);
    crate::bios::mscon::con_write_bytes(b" A");
    put_hex8(parsed.side);
    crate::bios::mscon::con_write_bytes(b" E");
    put_hex8(parsed.sectors);
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex16(parsed.status);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(parsed.exit_code);
    crate::bios::mscon::con_write_bytes(b" MN");
    put_hex16(parsed.message);
    crate::bios::mscon::con_write_bytes(b" K");
    put_hex8(parsed.substitutions as u8);
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
