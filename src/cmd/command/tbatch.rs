//! Batch commands from `CMD/COMMAND/TBATCH.ASM`.
//!
//! `IF EXIST` runs the rest of the line when the root name is present.
//! `CALL` with no batch file is message 1009. `GOTO` with no label is
//! message 1029. `FOR` runs the `DO` command once per item. `SHIFT` has
//! no parameter list at the prompt.

use crate::bios::mscon::con_write_bytes;

pub fn if_cmd(env: &mut super::tenv::Environment, tail: &[u8]) {
    let mut rest = skip_space(tail);
    let mut invert = false;
    if word(rest, b"NOT") {
        invert = true;
        rest = skip_space(&rest[3..]);
    }
    if word(rest, b"EXIST") {
        rest = skip_space(&rest[5..]);
        let name = token(rest);
        let cmd = skip_token(rest);
        let found = super::tcmd1a::name_exists(name);
        if found != invert && !blank(cmd) {
            env.dispatch(cmd);
        }
        return;
    }
    con_write_bytes(b"Syntax error\r\n");
}

pub fn call_cmd(_tail: &[u8]) {
    con_write_bytes(b"Batch file missing\r\n");
}

pub fn goto_cmd(_tail: &[u8]) {
    con_write_bytes(b"Label not found\r\n");
}

pub fn shift_cmd(_tail: &[u8]) {}

pub fn for_cmd(env: &mut super::tenv::Environment, tail: &[u8]) {
    let rest = skip_space(tail);
    let var_end = token_end(rest);
    let after_var = skip_space(&rest[var_end..]);
    if !word(after_var, b"IN") {
        con_write_bytes(b"Syntax error\r\n");
        return;
    }
    let after_in = skip_space(&after_var[2..]);
    if after_in.first() != Some(&b'(') {
        con_write_bytes(b"Syntax error\r\n");
        return;
    }
    let mut i = 1usize;
    while i < after_in.len() && after_in[i] != b')' {
        i += 1;
    }
    if i >= after_in.len() {
        con_write_bytes(b"Syntax error\r\n");
        return;
    }
    let items = &after_in[1..i];
    let after = skip_space(&after_in[i + 1..]);
    if !word(after, b"DO") {
        con_write_bytes(b"Syntax error\r\n");
        return;
    }
    let cmd = skip_space(&after[2..]);
    let mut item = items;
    while !blank(item) {
        item = skip_space(item);
        if blank(item) {
            break;
        }
        let _one = token(item);
        item = skip_token(item);
        if !blank(cmd) {
            env.dispatch(cmd);
        }
    }
}

fn word(tail: &[u8], lit: &[u8]) -> bool {
    if tail.len() < lit.len() || !tail[..lit.len()].eq_ignore_ascii_case(lit) {
        return false;
    }
    tail.len() == lit.len() || is_sep(tail[lit.len()])
}

fn is_sep(byte: u8) -> bool {
    byte == b' ' || byte == b'\t' || byte == b'\r' || byte == b'\n' || byte == 0
}

fn token(tail: &[u8]) -> &[u8] {
    let tail = skip_space(tail);
    &tail[..token_end(tail)]
}

fn token_end(tail: &[u8]) -> usize {
    let mut end = 0usize;
    while end < tail.len() && !is_sep(tail[end]) {
        end += 1;
    }
    end
}

fn skip_token(tail: &[u8]) -> &[u8] {
    let tail = skip_space(tail);
    &tail[token_end(tail)..]
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
