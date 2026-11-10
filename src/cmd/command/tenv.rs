//! `SET` from `CMD/COMMAND/TENV.ASM`.
//!
//! A blank tail is `DISP_ENV`. One `=` stores the name, uppercased, and the
//! value as typed. Any other number of `=` signs is message 1030.

use crate::bios::mscon::con_write_bytes;
use crate::cmd::command::envdata::{COMSPEC_KEY, ECOMSPEC};

const SLOTS: usize = 4;
const NAME_MAX: usize = 16;
const VALUE_MAX: usize = 32;

#[derive(Clone, Copy)]
struct Slot {
    name: [u8; NAME_MAX],
    name_len: u8,
    value: [u8; VALUE_MAX],
    value_len: u8,
    used: bool,
}

static mut VARS: [Slot; SLOTS] = [Slot {
    name: [0; NAME_MAX],
    name_len: 0,
    value: [0; VALUE_MAX],
    value_len: 0,
    used: false,
}; SLOTS];

static mut PROMPT: [u8; 64] = [0; 64];
static mut PROMPT_LEN: usize = 0;
static mut PROMPT_SET: bool = false;

/// `ADD_PROMPT`. `$P` is the current directory and `$G` is `>`.
pub fn prompt_cmd(tail: &[u8]) {
    let tail = skip_space(tail);
    if blank(tail) {
        unsafe { PROMPT_SET = false }
        return;
    }
    let mut end = 0usize;
    while end < tail.len() && tail[end] != b'\r' && tail[end] != b'\n' && tail[end] != 0 {
        end += 1;
    }
    let n = end.min(64);
    unsafe {
        PROMPT[..n].copy_from_slice(&tail[..n]);
        PROMPT_LEN = n;
        PROMPT_SET = true;
    }
    print_prompt();
}

fn print_prompt() {
    let (set, len) = unsafe { (PROMPT_SET, PROMPT_LEN) };
    if !set {
        con_write_bytes(b"A>");
        return;
    }
    let text = unsafe { &PROMPT[..len] };
    let mut i = 0usize;
    while i < text.len() {
        if text[i] == b'$' && i + 1 < text.len() {
            let code = up(text[i + 1]);
            if code == b'P' {
                con_write_bytes(super::tcmd1a::cwd_text());
            } else if code == b'G' {
                con_write_bytes(b">");
            } else if code == b'$' {
                con_write_bytes(b"$");
            }
            i += 2;
        } else {
            con_write_bytes(&text[i..i + 1]);
            i += 1;
        }
    }
    con_write_bytes(b"\r\n");
}

pub fn set_cmd(tail: &[u8]) {
    let tail = skip_space(tail);
    if blank(tail) {
        show_env();
        return;
    }
    let mut end = 0usize;
    while end < tail.len() && tail[end] != b'\r' && tail[end] != b'\n' && tail[end] != 0 {
        end += 1;
    }
    let line = &tail[..end];
    let mut eqs = 0u8;
    let mut eq_at = 0usize;
    let mut index = 0usize;
    while index < line.len() {
        if line[index] == b'=' {
            eqs = eqs.wrapping_add(1);
            eq_at = index;
        }
        index += 1;
    }
    if eqs != 1 || eq_at == 0 {
        con_write_bytes(b"Syntax error\r\n");
        return;
    }
    let name = &line[..eq_at];
    let value = &line[eq_at + 1..];
    if value.is_empty() {
        delete_name(name);
        return;
    }
    store_name(name, value);
}

fn show_env() {
    con_write_bytes(b"PATH=");
    let mut path = [0u8; 67];
    let n = super::tcmd2a::copy_path(&mut path);
    if n != 0 {
        con_write_bytes(&path[..n]);
    }
    con_write_bytes(b"\r\n");
    con_write_bytes(COMSPEC_KEY);
    con_write_bytes(ECOMSPEC);
    con_write_bytes(b"\r\n");
    let mut slot = 0usize;
    while slot < SLOTS {
        let (used, name_len, value_len) = unsafe {
            (
                VARS[slot].used,
                VARS[slot].name_len as usize,
                VARS[slot].value_len as usize,
            )
        };
        if used {
            con_write_bytes(unsafe { &VARS[slot].name[..name_len] });
            con_write_bytes(b"=");
            con_write_bytes(unsafe { &VARS[slot].value[..value_len] });
            con_write_bytes(b"\r\n");
        }
        slot += 1;
    }
}

fn store_name(name: &[u8], value: &[u8]) {
    let mut upper = [0u8; NAME_MAX];
    let nlen = name.len().min(NAME_MAX);
    let mut i = 0usize;
    while i < nlen {
        upper[i] = up(name[i]);
        i += 1;
    }
    let vlen = value.len().min(VALUE_MAX);
    let mut found = SLOTS;
    let mut free = SLOTS;
    let mut slot = 0usize;
    while slot < SLOTS {
        let (used, name_len) = unsafe { (VARS[slot].used, VARS[slot].name_len as usize) };
        if used && name_len == nlen && unsafe { VARS[slot].name[..nlen] == upper[..nlen] } {
            found = slot;
            break;
        }
        if !used && free == SLOTS {
            free = slot;
        }
        slot += 1;
    }
    let dest = if found != SLOTS { found } else { free };
    if dest == SLOTS {
        con_write_bytes(b"Out of environment space\r\n");
        return;
    }
    unsafe {
        VARS[dest].name[..nlen].copy_from_slice(&upper[..nlen]);
        VARS[dest].name_len = nlen as u8;
        VARS[dest].value[..vlen].copy_from_slice(&value[..vlen]);
        VARS[dest].value_len = vlen as u8;
        VARS[dest].used = true;
    }
}

fn delete_name(name: &[u8]) {
    let mut slot = 0usize;
    while slot < SLOTS {
        let (used, name_len) = unsafe { (VARS[slot].used, VARS[slot].name_len as usize) };
        if used && name_len == name.len() {
            let mut same = true;
            let mut i = 0usize;
            while i < name_len {
                if unsafe { VARS[slot].name[i] } != up(name[i]) {
                    same = false;
                    break;
                }
                i += 1;
            }
            if same {
                unsafe { VARS[slot].used = false }
                return;
            }
        }
        slot += 1;
    }
}

fn up(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte - 0x20
    } else {
        byte
    }
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
