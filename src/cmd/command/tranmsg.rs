//! Transient message blocks from `CMD/COMMAND/TRANMSG.ASM`.
//!
//! The display class starts as a utility message and the continuation flag is
//! clear. Each block is a message number, a substitution count, and then one
//! 11-byte substitution. The shared string block has one extra zero after it.
//! `PATH=`, `PROMPT=`, and `COMSPEC=` follow the table and are not terminated.

const UTILITY: u8 = 0xFF;
const BLOCK: u8 = 11;
const TEXT: u8 = 0x10;
const WORD: u8 = 0x21;
const DWORD: u8 = 0x31;
const HEX_WORD: u8 = 0x23;
const DATE_LONG: u8 = 0x34;
const DATE_SHORT: u8 = 0x24;
const TIME_COUNTRY: u8 = 0x05;
const TIME_FULL: u8 = 0x25;
const TIME_24: u8 = 0x26;
const RIGHT: u8 = 0x80;
const SPACE: u8 = b' ';
const ZERO: u8 = b'0';

#[derive(Clone, Copy)]
struct Slot {
    which: u8,
    kind: u8,
    widest: u8,
    narrowest: u8,
    pad: u8,
}

struct Message {
    number: u16,
    slots: [Slot; 2],
    count: u8,
    trailer: Option<u8>,
}

const EMPTY: Slot = Slot { which: 0, kind: 0, widest: 0, narrowest: 0, pad: 0 };

const fn plain(number: u16) -> Message {
    Message { number, slots: [EMPTY, EMPTY], count: 0, trailer: None }
}

const fn one(number: u16, slot: Slot) -> Message {
    Message { number, slots: [slot, EMPTY], count: 1, trailer: None }
}

const fn two(number: u16, first: Slot, second: Slot) -> Message {
    Message { number, slots: [first, second], count: 2, trailer: None }
}

const fn slot(which: u8, kind: u8, widest: u8, narrowest: u8, pad: u8) -> Slot {
    Slot { which, kind, widest, narrowest, pad }
}

fn messages() -> [Message; 71] {
    let copied = slot(1, RIGHT + WORD, 9, 9, SPACE);
    let page = slot(1, RIGHT + WORD, 5, 1, SPACE);
    let text = slot(1, TEXT, 128, 0, SPACE);
    let named = slot(1, TEXT, 128, 1, SPACE);
    [
        plain(1002),
        plain(1003),
        plain(1004),
        plain(1007),
        plain(1008),
        plain(1009),
        plain(1010),
        plain(1011),
        plain(1014),
        plain(1015),
        plain(1016),
        plain(1017),
        one(1018, copied),
        one(1019, copied),
        one(1020, slot(1, RIGHT + DWORD, 10, 10, SPACE)),
        plain(1021),
        one(1022, page),
        one(1023, page),
        one(1024, page),
        plain(1025),
        plain(1026),
        plain(1027),
        plain(1028),
        plain(1029),
        plain(1030),
        plain(1031),
        two(1032, slot(1, TEXT, 3, 3, SPACE), slot(2, DATE_LONG, 10, 10, SPACE)),
        plain(1033),
        one(1034, slot(1, TEXT, 8, 8, SPACE)),
        plain(1035),
        one(1036, slot(1, RIGHT + TIME_FULL, 12, 12, SPACE)),
        plain(1037),
        plain(1038),
        plain(1039),
        two(1040, slot(1, RIGHT + WORD, 1, 1, SPACE), slot(2, WORD, 2, 2, ZERO)),
        one(1041, slot(1, 0, 128, 1, SPACE)),
        two(1042, slot(1, 0, 128, 1, SPACE), slot(2, TEXT, 128, 1, SPACE)),
        two(1043, slot(1, RIGHT + HEX_WORD, 4, 4, ZERO), slot(2, RIGHT + HEX_WORD, 4, 4, ZERO)),
        plain(1044),
        plain(1045),
        plain(1046),
        plain(1047),
        one(1048, text),
        plain(1049),
        plain(1050),
        plain(1051),
        plain(1052),
        plain(1053),
        plain(1054),
        one(1055, named),
        one(1056, named),
        one(1057, named),
        plain(1059),
        plain(1060),
        plain(1061),
        plain(1062),
        one(1063, text),
        one(1064, text),
        one(1065, slot(1, RIGHT + DWORD, 10, 10, SPACE)),
        Message { number: 1066, slots: [text, EMPTY], count: 1, trailer: Some(0) },
        plain(1067),
        plain(1068),
        plain(1069),
        plain(1070),
        plain(1072),
        plain(1073),
        plain(1074),
        two(1075, slot(1, TEXT, 3, 3, SPACE), slot(2, DATE_LONG, 10, 8, SPACE)),
        one(1076, slot(1, RIGHT + TIME_24, 11, 11, SPACE)),
        two(1077, slot(1, RIGHT + DATE_SHORT, 10, 8, SPACE), slot(2, RIGHT + TIME_COUNTRY, 6, 6, SPACE)),
        plain(1078),
    ]
}

fn write_slot(slot: &Slot) {
    put_hex8(slot.which);
    put_hex8(slot.kind);
    put_hex8(slot.widest);
    put_hex8(slot.narrowest);
    put_hex8(slot.pad);
}

fn write_message(message: &Message) {
    crate::bios::mscon::con_write_bytes(b" M");
    put_hex16(message.number);
    put_hex8(message.count);
    let mut index = 0usize;
    while index < message.count as usize {
        write_slot(&message.slots[index]);
        index += 1;
    }
    if let Some(extra) = message.trailer {
        crate::bios::mscon::con_write_bytes(b" X");
        put_hex8(extra);
    }
}

fn write_name(label: &[u8], text: &[u8]) {
    crate::bios::mscon::con_write_bytes(label);
    let mut index = 0usize;
    while index < text.len() {
        put_hex8(text[index]);
        index += 1;
    }
}

pub fn show() {
    let table = messages();
    crate::bios::mscon::con_write_bytes(b"TRANMSG CL");
    put_hex8(UTILITY);
    crate::bios::mscon::con_write_bytes(b" CT00 EX0000 NS00 SZ");
    put_hex8(BLOCK);
    crate::bios::mscon::con_write_bytes(b" RS00 SG0000 ID00 FL");
    put_hex8(TEXT);
    crate::bios::mscon::con_write_bytes(b" MX80 MN00 PD20");
    let mut index = 0usize;
    while index < table.len() {
        write_message(&table[index]);
        index += 1;
    }
    write_name(b" PT", b"PATH=");
    write_name(b" PR", b"PROMPT=");
    write_name(b" CS", b"COMSPEC=");
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
