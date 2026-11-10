//! Data layout from `CMD/DEBUG/DEBDATA.ASM`.
//!
//! The relative read table is ten bytes. The expanded-memory page buffer is
//! 1024 zero bytes. Extended error state is the 22-byte device parameter
//! list. The create-process dword keeps offset 100h beside the segment.
//! Assembly bytes stay in front of the 80-byte list buffer, and the
//! breakpoint table is 50 bytes. The buffer pad ends 512 bytes after the
//! byte count.

struct Field {
    tag: &'static [u8],
    size: u16,
    value: Option<u16>,
}

const FIELDS: &[Field] = &[
    Field { tag: b"RL", size: 10, value: None },
    Field { tag: b"LB", size: 4, value: None },
    Field { tag: b"XM", size: 3, value: None },
    Field { tag: b"", size: 6, value: None },
    Field { tag: b"", size: 2, value: None },
    Field { tag: b"PG", size: 1024, value: Some(0) },
    Field { tag: b"FR", size: 2, value: None },
    Field { tag: b"", size: 8, value: None },
    Field { tag: b"OM", size: 1, value: None },
    Field { tag: b"", size: 4, value: None },
    Field { tag: b"FN", size: 1, value: Some(0) },
    Field { tag: b"FZ", size: 1, value: Some(0) },
    Field { tag: b"", size: 12, value: None },
    Field { tag: b"PV", size: 1, value: Some(0) },
    Field { tag: b"DP", size: 22, value: None },
    Field { tag: b"", size: 38, value: None },
    Field { tag: b"CR", size: 2, value: Some(0x0100) },
    Field { tag: b"BG", size: 2, value: None },
    Field { tag: b"", size: 4, value: None },
    Field { tag: b"BF", size: 1, value: None },
    Field { tag: b"", size: 14, value: None },
    Field { tag: b"AS", size: 7, value: None },
    Field { tag: b"BB", size: 80, value: None },
    Field { tag: b"PT", size: 50, value: None },
    Field { tag: b"", size: 17, value: None },
];

const BUFFER_LIMIT: u16 = 512;

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DEBDATA");
    let mut cursor = 0u16;
    let mut buffer = 0u16;
    let mut index = 0usize;
    while index < FIELDS.len() {
        let field = &FIELDS[index];
        if field.tag == b"BF" {
            buffer = cursor;
        }
        if !field.tag.is_empty() {
            crate::bios::mscon::con_write_bytes(b" ");
            crate::bios::mscon::con_write_bytes(field.tag);
            put_hex16(cursor);
            if let Some(value) = field.value {
                put_hex16(value);
            }
        }
        cursor = cursor.wrapping_add(field.size);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b" EN");
    put_hex16(buffer.wrapping_add(BUFFER_LIMIT));
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
