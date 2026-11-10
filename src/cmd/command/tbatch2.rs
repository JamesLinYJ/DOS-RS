//! Batch conditionals from `CMD/COMMAND/TBATCH2.ASM`.
//!
//! The keyword table is `NOT`, then `ERRORLEVEL`, then `EXIST`. Anything else
//! is a string compare. `ERRORLEVEL n` is true when the return code is at
//! least `n`. `NOT` inverts the result. `SHIFT` drops the first parameter.
//! A label matches in either case, and a short label must end on a space.

const NOT_WORD: &[u8] = b"NOT";
const LEVEL_WORD: &[u8] = b"ERRORLEVEL";
const EXIST_WORD: &[u8] = b"EXIST";

fn is_delim(byte: u8) -> bool {
    byte == b' ' || byte == b'=' || byte == b',' || byte == b';' || byte == b'\t' || byte == 0x0A
}

fn keyword_match(input: &[u8], word: &[u8]) -> bool {
    if input.len() < word.len() {
        return false;
    }
    let mut index = 0usize;
    while index < word.len() {
        let table = word[index];
        let candidate = input[index];
        if candidate != table && candidate != (table | 0x20) {
            return false;
        }
        index += 1;
    }
    true
}

/// `1` NOT, `2` ERRORLEVEL, `3` EXIST, `0` a string compare.
fn keyword(line: &[u8]) -> u8 {
    let table: &[(&[u8], u8)] = &[(NOT_WORD, 1), (LEVEL_WORD, 2), (EXIST_WORD, 3)];
    let mut index = 0usize;
    while index < table.len() {
        let (word, code) = table[index];
        index += 1;
        if keyword_match(line, word) {
            let next = if line.len() > word.len() { line[word.len()] } else { 0 };
            if is_delim(next) {
                return code;
            }
        }
    }
    0
}

fn strings_equal(line: &[u8]) -> Result<bool, ()> {
    let mut cursor = 0usize;
    let mut count = 0usize;
    let first = 0usize;
    let ended = loop {
        if cursor >= line.len() {
            return Err(());
        }
        let byte = line[cursor];
        cursor += 1;
        if byte == b'\r' {
            return Err(());
        }
        if is_delim(byte) {
            break byte;
        }
        count += 1;
    };
    let mut current = ended;
    loop {
        if current == b'=' {
            break;
        }
        if current == b'\r' {
            return Err(());
        }
        if cursor >= line.len() {
            return Err(());
        }
        current = line[cursor];
        cursor += 1;
    }
    if cursor >= line.len() || line[cursor] != b'=' {
        return Err(());
    }
    cursor += 1;
    while cursor < line.len() && is_delim(line[cursor]) {
        cursor += 1;
    }
    if cursor >= line.len() || line[cursor] == b'\r' {
        return Err(());
    }
    let mut left = first;
    let mut right = cursor;
    let mut remaining = count;
    let mut matched = true;
    while remaining != 0 {
        let left_byte = line[left];
        let right_byte = if right < line.len() { line[right] } else { 0 };
        left += 1;
        right += 1;
        remaining -= 1;
        if left_byte != right_byte {
            matched = false;
            break;
        }
    }
    if matched {
        let next = if right < line.len() { line[right] } else { 0 };
        return if is_delim(next) { Ok(true) } else { Ok(false) };
    }
    if right > 0 && line[right - 1] == b'\r' {
        return Err(());
    }
    Ok(false)
}

fn level_number(text: &[u8]) -> Result<u8, ()> {
    let mut value = 0u8;
    let mut cursor = 0usize;
    loop {
        if cursor >= text.len() {
            return Err(());
        }
        let byte = text[cursor];
        cursor += 1;
        if byte == b'\r' {
            return Err(());
        }
        if is_delim(byte) {
            return Ok(value);
        }
        let digit = byte.wrapping_sub(b'0');
        value = value.wrapping_mul(10).wrapping_add(digit);
    }
}

fn level_true(returned: u8, number: u8) -> bool {
    returned >= number
}

fn apply_not(matched: bool, invert: bool) -> bool {
    if invert { !matched } else { matched }
}

fn not_state(times: u8) -> (u8, bool) {
    let mut flag = 0u8;
    let mut count = 0u8;
    let mut left = times;
    while left != 0 {
        flag = !flag;
        count += 1;
        left -= 1;
    }
    (count, flag != 0)
}

fn goto_length(field: &[u8; 11]) -> u8 {
    let mut remaining = 11u8;
    let mut index = 0usize;
    let mut found = false;
    while remaining != 0 {
        remaining -= 1;
        if field[index] == b' ' {
            found = true;
            break;
        }
        index += 1;
    }
    if found {
        remaining += 1;
    }
    remaining.wrapping_sub(11).wrapping_neg()
}

fn label_matches(file: &[u8], label: &[u8]) -> bool {
    let mut index = 0usize;
    while index < label.len() {
        if index >= file.len() {
            return false;
        }
        let lowered = file[index] | 0x20;
        let raised = lowered.wrapping_sub(0x20);
        if lowered != label[index] && raised != label[index] {
            return false;
        }
        index += 1;
    }
    if label.len() >= 8 {
        return true;
    }
    let next = if index < file.len() { file[index] } else { 0 };
    next <= b' '
}

fn shift(parms: &mut [i16; 10], text: &[u8]) {
    let mut index = 0usize;
    while index < 9 {
        parms[index] = parms[index + 1];
        index += 1;
    }
    if parms[9] == -1 {
        return;
    }
    let mut cursor = parms[9] as usize;
    parms[9] = -1;
    loop {
        let byte = text[cursor];
        cursor += 1;
        if byte == b'\r' {
            break;
        }
    }
    if text[cursor] != 0 {
        parms[9] = cursor as i16;
    }
}

fn strip_call(line: &[u8]) -> &[u8] {
    let mut cursor = 0usize;
    while cursor < line.len() && is_delim(line[cursor]) {
        cursor += 1;
    }
    let start = cursor + 4;
    if start >= line.len() { b"" } else { &line[start..] }
}

pub fn show() {
    let mut field = [b' '; 11];
    field[0] = b'F';
    field[1] = b'O';
    field[2] = b'O';
    let long = [b'A'; 11];
    let mut parms = [0, 2, 4, -1, -1, -1, -1, -1, -1, -1];
    shift(&mut parms, b"A\rB\rC\r\0");
    let mut tenth = [-1, -1, -1, -1, -1, -1, -1, -1, -1, 0];
    shift(&mut tenth, b"J\rK\r\0");
    let called = strip_call(b"CALL FOO");
    let (one_count, one_invert) = not_state(1);
    let (two_count, two_invert) = not_state(2);

    crate::bios::mscon::con_write_bytes(b"TBATCH2 KW");
    put_hex8(keyword(b"not "));
    crate::bios::mscon::con_write_bytes(b" KW");
    put_hex8(keyword(b"errorlevel "));
    crate::bios::mscon::con_write_bytes(b" KW");
    put_hex8(keyword(b"exist "));
    crate::bios::mscon::con_write_bytes(b" KW");
    put_hex8(keyword(b"NOTE "));
    crate::bios::mscon::con_write_bytes(b" EQ");
    put_hex8(u8::from(strings_equal(b"FOO == FOO ").unwrap_or(false)));
    crate::bios::mscon::con_write_bytes(b" EQ");
    put_hex8(u8::from(strings_equal(b"FOO == BAR ").unwrap_or(false)));
    crate::bios::mscon::con_write_bytes(b" SY");
    put_hex8(u8::from(strings_equal(b"FOO=FOO ").is_err()));
    crate::bios::mscon::con_write_bytes(b" LV");
    put_hex8(u8::from(level_true(5, level_number(b"5 ").unwrap_or(0))));
    crate::bios::mscon::con_write_bytes(b" LV");
    put_hex8(u8::from(level_true(4, level_number(b"5 ").unwrap_or(0))));
    crate::bios::mscon::con_write_bytes(b" NV");
    put_hex8(u8::from(apply_not(
        level_true(4, level_number(b"5 ").unwrap_or(0)),
        true,
    )));
    crate::bios::mscon::con_write_bytes(b" NC");
    put_hex8(one_count);
    put_hex8(u8::from(one_invert));
    crate::bios::mscon::con_write_bytes(b" NC");
    put_hex8(two_count);
    put_hex8(u8::from(two_invert));
    crate::bios::mscon::con_write_bytes(b" GL");
    put_hex8(goto_length(&field));
    crate::bios::mscon::con_write_bytes(b" GL");
    put_hex8(goto_length(&long));
    crate::bios::mscon::con_write_bytes(b" GL");
    put_hex8(goto_length(&[b' '; 11]));
    crate::bios::mscon::con_write_bytes(b" LB");
    put_hex8(u8::from(label_matches(b"foo ", b"FOO")));
    crate::bios::mscon::con_write_bytes(b" LB");
    put_hex8(u8::from(label_matches(b"FOOX", b"FOO")));
    crate::bios::mscon::con_write_bytes(b" SH");
    put_hex16(parms[0] as u16);
    crate::bios::mscon::con_write_bytes(b" NX");
    put_hex16(tenth[9] as u16);
    crate::bios::mscon::con_write_bytes(b" CA");
    put_hex8(called[0]);
    put_hex8(called[1]);
    put_hex8(called[2]);
    put_hex8(called[3]);
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
