//! `GetSet_XA` from `DOS/EXTATTR.ASM`.
//!
//! Kind 2 reads attributes and kind 3 reads names. A zero-length buffer asks
//! for the maximum size, which this DOS 4 build reports as 2. A one-byte
//! buffer is too small and returns without a size. Any other kind is accepted
//! without writing the list. The real attribute table is still commented out
//! in the source, so the list word stored for a usable buffer is 0.

const GET_ATTRIBUTES: u8 = 2;
const GET_NAMES: u8 = 3;

pub enum Outcome {
    Sized { count: u16, list_word: Option<u16> },
    Accepted,
}

/// Get or set extended attributes for one handle.
pub fn get_set_xa(kind: u8, buffer_size: u16, list: &mut [u8]) -> Outcome {
    if kind == GET_ATTRIBUTES {
        return get_attributes(buffer_size, list);
    }
    if kind == GET_NAMES {
        if buffer_size == 0 {
            return Outcome::Sized {
                count: 2,
                list_word: None,
            };
        }
        return usable_buffer(buffer_size, list);
    }
    Outcome::Accepted
}

fn get_attributes(buffer_size: u16, list: &mut [u8]) -> Outcome {
    if buffer_size == 0 {
        return Outcome::Sized {
            count: 2,
            list_word: None,
        };
    }
    usable_buffer(buffer_size, list)
}

fn usable_buffer(buffer_size: u16, list: &mut [u8]) -> Outcome {
    if buffer_size == 1 {
        return Outcome::Accepted;
    }
    if list.len() >= 2 {
        list[0] = 0;
        list[1] = 0;
    }
    Outcome::Sized {
        count: 2,
        list_word: Some(0),
    }
}

pub fn show() {
    let mut list = [0xFFu8; 4];
    let max_size = get_set_xa(GET_ATTRIBUTES, 0, &mut list);
    let too_small = get_set_xa(GET_ATTRIBUTES, 1, &mut list);
    let stored = get_set_xa(GET_ATTRIBUTES, 4, &mut list);
    let names = get_set_xa(GET_NAMES, 0, &mut list);
    let set = get_set_xa(4, 4, &mut list);
    crate::bios::mscon::con_write_bytes(b"XA G0 ");
    write_outcome(&max_size);
    crate::bios::mscon::con_write_bytes(b" G1 ");
    write_outcome(&too_small);
    crate::bios::mscon::con_write_bytes(b" G4 ");
    write_outcome(&stored);
    crate::bios::mscon::con_write_bytes(b" N0 ");
    write_outcome(&names);
    crate::bios::mscon::con_write_bytes(b" S ");
    write_outcome(&set);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write_outcome(outcome: &Outcome) {
    match outcome {
        Outcome::Sized {
            count,
            list_word: None,
        } => {
            crate::bios::mscon::con_write_bytes(b"C");
            put_hex16(*count);
        }
        Outcome::Sized {
            count,
            list_word: Some(word),
        } => {
            crate::bios::mscon::con_write_bytes(b"C");
            put_hex16(*count);
            crate::bios::mscon::con_write_bytes(b" L");
            put_hex16(*word);
        }
        Outcome::Accepted => crate::bios::mscon::con_write_bytes(b"OK"),
    }
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
