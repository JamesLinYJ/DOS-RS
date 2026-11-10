//! `DATE` and `TIME` from `CMD/COMMAND/TPIPE.ASM`.
//!
//! A blank tail prints message 1032 or 1036 from INT 21h AH=2Ah or AH=2Ch.
//! A date or time on the line is AH=2Bh or AH=2Dh. There is no keyboard, so
//! the prompt that would follow a blank line is the Enter that ends it.

use crate::bios::mscon::con_write_bytes;

const WEEK: &[u8] = b"SunMonTueWedThuFriSat";

pub fn date_cmd(tail: &[u8]) {
    let tail = skip_space(tail);
    if blank(tail) {
        let (year, month, day, week) = get_date();
        con_write_bytes(b"Current date is ");
        let index = (week as usize) * 3;
        con_write_bytes(&WEEK[index..index + 3]);
        con_write_bytes(b" ");
        put_2(month);
        con_write_bytes(b"-");
        put_2(day);
        con_write_bytes(b"-");
        put_year(year);
        con_write_bytes(b"\r\n");
        return;
    }
    if let Some((month, day, year)) = parse_date(tail) {
        let _ = set_date(year, month, day);
    } else {
        con_write_bytes(b"\r\nInvalid date\r\n");
    }
}

pub fn time_cmd(tail: &[u8]) {
    let tail = skip_space(tail);
    if blank(tail) {
        let (hour, minute, second, hundredth) = get_time();
        con_write_bytes(b"Current time is ");
        con_write_bytes(b" ");
        put_2(hour);
        con_write_bytes(b":");
        put_2(minute);
        con_write_bytes(b":");
        put_2(second);
        con_write_bytes(b".");
        put_2(hundredth);
        con_write_bytes(b"\r\n");
        return;
    }
    if let Some((hour, minute, second, hundredth)) = parse_time(tail) {
        let _ = set_time(hour, minute, second, hundredth);
    } else {
        con_write_bytes(b"\r\nInvalid time\r\n");
    }
}

fn get_date() -> (u16, u8, u8, u8) {
    crate::int21::current_date()
}

fn set_date(year: u16, month: u8, day: u8) -> u8 {
    crate::int21::store_date(year, month, day)
}

fn get_time() -> (u8, u8, u8, u8) {
    crate::int21::current_time()
}

fn set_time(hour: u8, minute: u8, second: u8, hundredth: u8) -> u8 {
    crate::int21::store_time(hour, minute, second, hundredth)
}

fn parse_date(tail: &[u8]) -> Option<(u8, u8, u16)> {
    let (month, rest) = number(tail)?;
    let rest = sep(rest)?;
    let (day, rest) = number(rest)?;
    let rest = sep(rest)?;
    let (year_raw, rest) = number(rest)?;
    if !blank(rest) || month > 255 || day > 255 {
        return None;
    }
    let year = if year_raw < 100 {
        if year_raw >= 80 {
            1900 + year_raw
        } else {
            2000 + year_raw
        }
    } else {
        year_raw
    };
    Some((month as u8, day as u8, year))
}

fn parse_time(tail: &[u8]) -> Option<(u8, u8, u8, u8)> {
    let (hour, rest) = number(tail)?;
    let rest = colon(rest)?;
    let (minute, rest) = number(rest)?;
    let (second, hundredth, rest) = if rest.first() == Some(&b':') {
        let rest = colon(rest)?;
        let (second, rest) = number(rest)?;
        if rest.first() == Some(&b'.') {
            let (hundredth, rest) = number(&rest[1..])?;
            (second, hundredth, rest)
        } else {
            (second, 0, rest)
        }
    } else {
        (0, 0, rest)
    };
    if !blank(rest) || hour > 255 || minute > 255 || second > 255 || hundredth > 255 {
        return None;
    }
    Some((hour as u8, minute as u8, second as u8, hundredth as u8))
}

fn number(tail: &[u8]) -> Option<(u16, &[u8])> {
    let mut i = 0usize;
    let mut value = 0u16;
    if i >= tail.len() || !tail[i].is_ascii_digit() {
        return None;
    }
    while i < tail.len() && tail[i].is_ascii_digit() {
        value = value.saturating_mul(10).saturating_add(u16::from(tail[i] - b'0'));
        i += 1;
    }
    Some((value, &tail[i..]))
}

fn sep(tail: &[u8]) -> Option<&[u8]> {
    if tail.first() == Some(&b'-') || tail.first() == Some(&b'/') {
        Some(&tail[1..])
    } else {
        None
    }
}

fn colon(tail: &[u8]) -> Option<&[u8]> {
    if tail.first() == Some(&b':') {
        Some(&tail[1..])
    } else {
        None
    }
}

fn put_2(value: u8) {
    con_write_bytes(&[b'0' + value / 10, b'0' + value % 10]);
}

fn put_year(year: u16) {
    let mut buf = [0u8; 4];
    let mut n = year;
    let mut i = 4usize;
    while i != 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    con_write_bytes(&buf);
}

fn skip_space(tail: &[u8]) -> &[u8] {
    let mut i = 0usize;
    while i < tail.len() && (tail[i] == b' ' || tail[i] == b'\t') {
        i += 1;
    }
    &tail[i..]
}

fn blank(tail: &[u8]) -> bool {
    for &byte in tail {
        if byte == b'\r' || byte == b'\n' || byte == 0 {
            return true;
        }
        if byte != b' ' && byte != b'\t' {
            return false;
        }
    }
    true
}
