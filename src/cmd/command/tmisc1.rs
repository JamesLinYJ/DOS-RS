//! `findcom` from `CMD/COMMAND/TMISC1.ASM`.
//!
//! The scan walks `COMTAB` until the name length and the bytes match.
//! `VER` has no switch bit, so a `/` in the tail is rejected. The command
//! that matches is the only one run.

use super::tdata::{COMTAB, F_SWITCH_ALLOWED};
use super::{tcmd2a, tenv, tpipe, tucode};

pub fn dispatch(line: &[u8]) -> bool {
    let (name, rest) = split_name(line);
    let Some(flags) = find_com(name) else {
        return false;
    };
    if flags & F_SWITCH_ALLOWED == 0 && rest.contains(&b'/') {
        return false;
    }
    if name.eq_ignore_ascii_case(b"VER") {
        tcmd2a::version();
        return true;
    }
    if name.eq_ignore_ascii_case(b"PATH") {
        tcmd2a::path_cmd(rest);
        return true;
    }
    if name.eq_ignore_ascii_case(b"CLS") {
        tcmd2a::cls();
        return true;
    }
    if name.eq_ignore_ascii_case(b"ECHO") {
        tucode::echo(rest);
        return true;
    }
    if name.eq_ignore_ascii_case(b"BREAK") {
        tucode::break_cmd(rest);
        return true;
    }
    if name.eq_ignore_ascii_case(b"VERIFY") {
        tucode::verify_cmd(rest);
        return true;
    }
    if name.eq_ignore_ascii_case(b"SET") {
        tenv::set_cmd(rest);
        return true;
    }
    if name.eq_ignore_ascii_case(b"DATE") {
        tpipe::date_cmd(rest);
        return true;
    }
    if name.eq_ignore_ascii_case(b"TIME") {
        tpipe::time_cmd(rest);
        return true;
    }
    false
}

fn find_com(name: &[u8]) -> Option<u8> {
    for (entry, flags) in COMTAB {
        if entry.len() == name.len() && entry.eq_ignore_ascii_case(name) {
            return Some(*flags);
        }
    }
    None
}

fn split_name(line: &[u8]) -> (&[u8], &[u8]) {
    let mut start = 0usize;
    while start < line.len() && line[start] == b' ' {
        start += 1;
    }
    let mut end = start;
    while end < line.len() && line[end] != b' ' && line[end] != b'\r' && line[end] != b'\n' {
        end += 1;
    }
    (&line[start..end], &line[end..])
}
