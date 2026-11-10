//! Resident reload from `CMD/COMMAND/COMMAND2.ASM`.
//!
//! A transient command needs its own size plus 32 paragraphs. The usable
//! buffer is the next 64K boundary when that boundary still leaves 64K inside
//! the allocation. A drive is removable only when the media call succeeds and
//! bit 0 is clear. Handles 2, 3, and 4 stay open; the rest are closed.

const HANDLES: u8 = 20;
const EXTRA: u16 = 0x20;
const BOUNDARY: u16 = 0x1000;

fn fatal(permanent: bool, single: u16) -> u8 {
    if !permanent {
        return 0;
    }
    if single != 0 {
        return 2;
    }
    1
}

fn return_code(external: u8, status: u16) -> u16 {
    if external == 0 { 0 } else { status }
}

fn transient_paragraphs(bytes: u16) -> u16 {
    bytes.wrapping_add(15) >> 4
}

fn block_fits(transient: u16, largest: u16) -> bool {
    largest >= transient.wrapping_add(EXTRA)
}

fn usable_buffer(resident: u16, paragraphs: u16) -> u16 {
    let high = resident & 0xF000;
    let aligned = high.wrapping_add(BOUNDARY);
    if aligned < high {
        return resident;
    }
    let limit = resident.wrapping_add(paragraphs);
    if limit <= aligned {
        return resident;
    }
    let usable = limit.wrapping_sub(aligned);
    if usable < BOUNDARY {
        return resident;
    }
    aligned
}

/// `REMCHECK`. A failed media call is treated as a fixed disk.
pub fn media_removable(call_ok: bool, status: u16) -> bool {
    if !call_ok {
        return false;
    }
    (status & 1) == 0
}

fn close_count(saved_input: u8, saved_output: u8, input: u8, output: u8) -> (u8, u8) {
    let mut closed = HANDLES - 5;
    let restored = if input == saved_input {
        input
    } else {
        closed += 1;
        saved_input
    };
    if output != saved_output {
        closed += 1;
    }
    (closed, restored)
}

fn directory_char(switch_char: u8, current: u8) -> u8 {
    if switch_char == b'/' { b'\\' } else { current }
}

fn single_finished(single: u16) -> bool {
    single == 0xFFFF
}

fn checksum_matches(found: u16, recorded: u16) -> bool {
    found == recorded
}

fn verify_value(stored: u16) -> Option<u8> {
    if stored == 0xFFFF {
        None
    } else {
        Some(stored as u8)
    }
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"CMD2 ");
    put_hex8(fatal(false, 0));
    put_hex8(fatal(true, 0));
    put_hex8(fatal(true, 5));
    put_hex8(fatal(false, 5));
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex16(return_code(0, 9));
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex16(return_code(1, 9));
    crate::bios::mscon::con_write_bytes(b" S");
    put_hex16(0x81);
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex16(64 * 2);
    crate::bios::mscon::con_write_bytes(b" SZ");
    put_hex16(transient_paragraphs(1));
    crate::bios::mscon::con_write_bytes(b" SZ");
    put_hex16(transient_paragraphs(16));
    crate::bios::mscon::con_write_bytes(b" SZ");
    put_hex16(transient_paragraphs(17));
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex8(u8::from(block_fits(0x10, 0x2F)));
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex8(u8::from(block_fits(0x10, 0x30)));
    crate::bios::mscon::con_write_bytes(b" AL");
    put_hex16(usable_buffer(0x2000, 0x2000));
    crate::bios::mscon::con_write_bytes(b" AL");
    put_hex16(usable_buffer(0x2000, 0x0800));
    crate::bios::mscon::con_write_bytes(b" AL");
    put_hex16(usable_buffer(0xF000, 0x2000));
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex8(u8::from(media_removable(false, 0)));
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex8(u8::from(media_removable(true, 0)));
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex8(u8::from(media_removable(true, 1)));
    let (closed, input) = close_count(1, 2, 5, 2);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(closed);
    crate::bios::mscon::con_write_bytes(b" IN");
    put_hex8(input);
    crate::bios::mscon::con_write_bytes(b" D");
    put_hex8(directory_char(b'/', b'/'));
    crate::bios::mscon::con_write_bytes(b" D");
    put_hex8(directory_char(b'-', b'/'));
    crate::bios::mscon::con_write_bytes(b" VF");
    put_hex8(verify_value(0xFFFF).unwrap_or(0));
    crate::bios::mscon::con_write_bytes(b" VF");
    put_hex8(verify_value(1).unwrap_or(0));
    crate::bios::mscon::con_write_bytes(b" SC");
    put_hex8(u8::from(single_finished(0)));
    crate::bios::mscon::con_write_bytes(b" SC");
    put_hex8(u8::from(single_finished(0xFFFF)));
    crate::bios::mscon::con_write_bytes(b" SM");
    put_hex8(u8::from(checksum_matches(0x1234, 0x1234)));
    crate::bios::mscon::con_write_bytes(b" SM");
    put_hex8(u8::from(checksum_matches(0x1234, 0x1235)));
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
