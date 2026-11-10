//! Command parameters from `CMD/COMP/COMPPAR.ASM`.
//!
//! Two optional filespecs are allowed and no switches. A returned ordinal that
//! matches the parameter count is the first name. Any other ordinal is the
//! second name, which is how a typed second name is steered away from the
//! first. End of line finishes with no error. Any other parser code is a
//! parameter error: leading spaces are skipped, the token ends at the next
//! space, and the exit code is 1.

const END_OF_LINE: u16 = 0xFFFF;
const FIRST: u16 = 1;
const SECOND: u16 = 2;
const PATH_LIMIT: u16 = 64;
const FILE_SPEC: u16 = 0x0201;
const FILE_CAPS: u16 = 0x0001;
const RESULT_FILE: u8 = 5;
const PARAMETER_ERROR: u8 = 1;

enum Reply {
    Finished,
    Name { ordinal: u16, text: &'static [u8] },
    Rejected { code: u16, text: &'static [u8], stop: usize },
}

struct Stored {
    first: [u8; 16],
    first_len: usize,
    second: [u8; 16],
    second_len: usize,
    failed: bool,
    code: u16,
    shown: [u8; 16],
    shown_len: usize,
}

fn copy_name(text: &[u8], dest: &mut [u8; 16]) -> usize {
    let mut used = 0usize;
    while used < text.len() && text[used] != 0 && used < dest.len() {
        dest[used] = text[used];
        used += 1;
    }
    used
}

fn visible_token(text: &[u8], stop: usize) -> (usize, usize) {
    let mut index = 0usize;
    while index < stop && index < text.len() && text[index] == b' ' {
        index += 1;
    }
    let start = index;
    while index < stop && index < text.len() && text[index] != b' ' {
        index += 1;
    }
    (start, index)
}

fn collect(replies: &[Reply], parameter_count: u16) -> Stored {
    let mut stored = Stored {
        first: [0; 16],
        first_len: 0,
        second: [0; 16],
        second_len: 0,
        failed: false,
        code: 0,
        shown: [0; 16],
        shown_len: 0,
    };
    let mut index = 0usize;
    while index < replies.len() {
        match &replies[index] {
            Reply::Finished => break,
            Reply::Rejected { code, text, stop } => {
                let (start, end) = visible_token(text, *stop);
                let mut used = 0usize;
                while start + used < end && used < stored.shown.len() {
                    stored.shown[used] = text[start + used];
                    used += 1;
                }
                stored.shown_len = used;
                stored.failed = true;
                stored.code = *code;
                break;
            }
            Reply::Name { ordinal, text } => {
                if *ordinal == parameter_count {
                    stored.first_len = copy_name(text, &mut stored.first);
                } else {
                    stored.second_len = copy_name(text, &mut stored.second);
                }
            }
        }
        index += 1;
    }
    stored
}

fn write_bytes(bytes: &[u8], len: usize) {
    let mut index = 0usize;
    while index < len {
        put_hex8(bytes[index]);
        index += 1;
    }
}

fn write_stored(stored: &Stored) {
    if stored.failed {
        crate::bios::mscon::con_write_bytes(b" EX");
        put_hex16(stored.code);
        crate::bios::mscon::con_write_bytes(b" TK");
        write_bytes(&stored.shown, stored.shown_len);
        crate::bios::mscon::con_write_bytes(b" LV");
        put_hex8(PARAMETER_ERROR);
        return;
    }
    crate::bios::mscon::con_write_bytes(b" P1");
    write_bytes(&stored.first, stored.first_len);
    crate::bios::mscon::con_write_bytes(b" P2");
    write_bytes(&stored.second, stored.second_len);
    crate::bios::mscon::con_write_bytes(b" DN");
}

pub fn show() {
    let command = [
        Reply::Name { ordinal: 1, text: b"FOO" },
        Reply::Name { ordinal: 2, text: b"BAR" },
        Reply::Finished,
    ];
    let typed_second = [Reply::Name { ordinal: 1, text: b"NAME" }, Reply::Finished];
    let rejected = [Reply::Rejected { code: 9, text: b"  BAD*", stop: 6 }];
    crate::bios::mscon::con_write_bytes(b"COMPPAR MN00 MX02 SW00 KW00 FL");
    put_hex16(FILE_SPEC);
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex16(FILE_CAPS);
    crate::bios::mscon::con_write_bytes(b" TY");
    put_hex8(RESULT_FILE);
    crate::bios::mscon::con_write_bytes(b" TGFF NV00 AT0081 OC0000 PC0000 PL");
    put_hex16(PATH_LIMIT);
    write_stored(&collect(&command, FIRST));
    write_stored(&collect(&typed_second, SECOND));
    write_stored(&collect(&rejected, FIRST));
    crate::bios::mscon::con_write_bytes(b" EL");
    put_hex16(END_OF_LINE);
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
