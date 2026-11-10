//! Command dispatch from `CMD/COMMAND/TCODE.ASM`.
//!
//! A finished single command exits before another prompt. Pipes outrank `FOR`,
//! which outranks batch, which outranks a single command. The free buffer is
//! capped at 64K and then rounded down to 512 bytes. A wildcard is not a
//! command. `C:` with nothing after it changes the drive.

const SINGLE_LIMIT: u16 = 0xEFFF;

fn restore_verify(saved: u16) -> Option<u8> {
    if saved == 0xFFFF { None } else { Some(saved as u8) }
}

fn single_finished(single: u16) -> bool {
    single == 0xFFFF
}

fn skip_for_single(single: u16) -> bool {
    if single == 0 {
        return false;
    }
    single < SINGLE_LIMIT
}

fn restore_directory(just_loaded: bool, marked: bool) -> bool {
    if just_loaded {
        return false;
    }
    marked
}

fn drop_pipes(files: u8, still_piping: u8) -> bool {
    if files == 0 {
        return false;
    }
    still_piping == 0
}

fn buffer_bytes(paragraphs: u16) -> u16 {
    let wide = u32::from(paragraphs) * 16;
    let capped = if wide > 0xFFFF { 0xFFFF } else { wide as u16 };
    if capped <= 512 { capped } else { capped & 0xFE00 }
}

fn leading_break(echo: bool, single: u16, piping: bool, for_loop: bool, batch: bool) -> bool {
    if !echo {
        return false;
    }
    if skip_for_single(single) {
        return false;
    }
    if piping {
        return false;
    }
    if for_loop {
        return false;
    }
    !batch
}

fn show_prompt(echo: bool, single: u16, for_loop: bool, batch: bool) -> bool {
    if !echo {
        return false;
    }
    if skip_for_single(single) {
        return false;
    }
    if for_loop {
        return false;
    }
    !batch
}

fn next_source(piping: bool, for_loop: bool, batch: bool, single: u16) -> u8 {
    if piping {
        return 1;
    }
    if for_loop {
        return 2;
    }
    if batch {
        return 3;
    }
    if single != 0 { 4 } else { 0 }
}

fn command_class(piped: bool, parsed: bool, wildcard: bool, count: u16, length: u16) -> u8 {
    if piped {
        return 1;
    }
    if !parsed || wildcard {
        return 2;
    }
    if count == 0 || length == 0 { 3 } else { 0 }
}

fn drive_change(name: &[u8], invalid: bool, command_present: bool) -> u16 {
    if name.len() < 2 || name[1] != b':' {
        return 0;
    }
    if invalid {
        return 0xFF;
    }
    if command_present {
        return 0;
    }
    u16::from((name[0] & !0x20).wrapping_sub(b'A'))
}

fn name_length(name: &[u8]) -> u8 {
    let mut remaining = 9u8;
    let mut index = 0usize;
    while remaining != 0 {
        remaining -= 1;
        let byte = if index < name.len() { name[index] } else { b' ' };
        index += 1;
        if byte == b' ' {
            break;
        }
    }
    8u8.wrapping_sub(remaining)
}

fn tail_length(tail: &[u8]) -> u8 {
    let mut count = 0u16;
    let mut index = 0usize;
    loop {
        let byte = if index < tail.len() { tail[index] } else { b'\r' };
        index += 1;
        count = count.wrapping_add(1);
        if byte == b'\r' {
            break;
        }
    }
    !(count.wrapping_neg() as u8)
}

fn other_switches(command_switches: u16, line_switches: u16) -> u16 {
    !command_switches & line_switches
}

fn after_empty(batch: bool, finished_single: bool) -> u8 {
    if batch {
        if finished_single { 1 } else { 2 }
    } else if finished_single {
        3
    } else {
        0
    }
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"TCODE VF");
    put_hex8(restore_verify(0xFFFF).unwrap_or(0));
    crate::bios::mscon::con_write_bytes(b" VF");
    put_hex8(restore_verify(1).unwrap_or(0));
    crate::bios::mscon::con_write_bytes(b" SX");
    put_hex8(u8::from(single_finished(0xFFFF)));
    crate::bios::mscon::con_write_bytes(b" SX");
    put_hex8(u8::from(single_finished(0)));
    crate::bios::mscon::con_write_bytes(b" RD");
    put_hex8(u8::from(restore_directory(true, true)));
    crate::bios::mscon::con_write_bytes(b" RD");
    put_hex8(u8::from(restore_directory(false, false)));
    crate::bios::mscon::con_write_bytes(b" RD");
    put_hex8(u8::from(restore_directory(false, true)));
    crate::bios::mscon::con_write_bytes(b" PP");
    put_hex8(u8::from(drop_pipes(0, 0)));
    crate::bios::mscon::con_write_bytes(b" PP");
    put_hex8(u8::from(drop_pipes(1, 1)));
    crate::bios::mscon::con_write_bytes(b" PP");
    put_hex8(u8::from(drop_pipes(1, 0)));
    crate::bios::mscon::con_write_bytes(b" BY");
    put_hex16(buffer_bytes(32));
    crate::bios::mscon::con_write_bytes(b" BY");
    put_hex16(buffer_bytes(64));
    crate::bios::mscon::con_write_bytes(b" BY");
    put_hex16(buffer_bytes(4096));
    crate::bios::mscon::con_write_bytes(b" BY");
    put_hex16(buffer_bytes(0));
    crate::bios::mscon::con_write_bytes(b" CR");
    put_hex8(u8::from(leading_break(true, 0, true, false, false)));
    crate::bios::mscon::con_write_bytes(b" CR");
    put_hex8(u8::from(leading_break(true, 0, false, false, false)));
    crate::bios::mscon::con_write_bytes(b" PR");
    put_hex8(u8::from(show_prompt(false, 0, false, false)));
    crate::bios::mscon::con_write_bytes(b" PR");
    put_hex8(u8::from(show_prompt(true, 0, false, false)));
    crate::bios::mscon::con_write_bytes(b" PR");
    put_hex8(u8::from(show_prompt(true, 1, false, false)));
    crate::bios::mscon::con_write_bytes(b" PR");
    put_hex8(u8::from(show_prompt(true, 0, true, false)));
    crate::bios::mscon::con_write_bytes(b" PR");
    put_hex8(u8::from(show_prompt(true, 0, false, true)));
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(next_source(true, true, true, 1));
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(next_source(false, true, true, 1));
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(next_source(false, false, true, 1));
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(next_source(false, false, false, 1));
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(next_source(false, false, false, 0));
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(command_class(true, true, false, 1, 1));
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(command_class(false, false, false, 1, 1));
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(command_class(false, true, true, 1, 1));
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(command_class(false, true, false, 0, 1));
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(command_class(false, true, false, 1, 0));
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(command_class(false, true, false, 1, 3));
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex16(drive_change(b"C:", true, false));
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex16(drive_change(b"C:", false, false));
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex16(drive_change(b"C:DIR", false, true));
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex16(drive_change(b"DIR", false, true));
    crate::bios::mscon::con_write_bytes(b" NM");
    put_hex8(name_length(b"DIR"));
    crate::bios::mscon::con_write_bytes(b" NM");
    put_hex8(name_length(b"COMMAND"));
    crate::bios::mscon::con_write_bytes(b" TL");
    put_hex8(tail_length(b"X\r"));
    crate::bios::mscon::con_write_bytes(b" TL");
    put_hex8(tail_length(b"\r"));
    crate::bios::mscon::con_write_bytes(b" SW");
    put_hex16(other_switches(0x000F, 0x00FF));
    crate::bios::mscon::con_write_bytes(b" NU");
    put_hex8(after_empty(true, true));
    crate::bios::mscon::con_write_bytes(b" NU");
    put_hex8(after_empty(true, false));
    crate::bios::mscon::con_write_bytes(b" NU");
    put_hex8(after_empty(false, true));
    crate::bios::mscon::con_write_bytes(b" NU");
    put_hex8(after_empty(false, false));
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
