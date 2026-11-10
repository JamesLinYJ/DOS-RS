//! Resident command executor.
//!
//! The transient shell reads a batch file and passes each line here. Built-ins
//! edit the mutable volume. A name that is not a built-in is loaded and run.

use super::disk;
use super::memory::GuestRam;
use super::run::Session;
use super::state::CpuState;

const ROWS: usize = 8;

#[derive(Clone, Copy)]
struct Binding {
    name: [u8; 8],
    value: [u8; 16],
    value_len: usize,
    live: bool,
}

static mut ENV: [Binding; ROWS] = [Binding {
    name: [0; 8],
    value: [0; 16],
    value_len: 0,
    live: false,
}; ROWS];

pub fn reset() {
    let env = env();
    let mut index = 0usize;
    while index < env.len() {
        env[index].live = false;
        index += 1;
    }
}

pub(super) fn invoke(cpu: &CpuState, ram: &mut GuestRam, session: &mut Session) {
    let mut line = [0u8; 80];
    let mut length = 0usize;
    let mut offset = cpu.wide(super::state::Slot::Data);
    let segment = cpu.selector(super::state::Segment::Data);
    while length < line.len() {
        let Some(byte) = ram.load(segment, offset) else {
            break;
        };
        if byte == 0 {
            break;
        }
        line[length] = byte;
        length += 1;
        offset = offset.wrapping_add(1);
    }
    execute(&line[..length], ram, session, 0);
}

fn execute(line: &[u8], ram: &mut GuestRam, session: &mut Session, depth: u8) {
    let line = trim(line);
    if line.is_empty() || depth > 4 {
        return;
    }
    let (verb, rest) = split(line);
    let mut name = [0u8; 8];
    let count = verb.len().min(name.len());
    name[..count].copy_from_slice(&verb[..count]);
    upper(&mut name[..count]);
    if name.starts_with(b"VER") {
        say(b"CMD VER 4.00\r\n");
    } else if name.starts_with(b"ECHO") {
        let mut expanded = [0u8; 80];
        let length = expand(rest, &mut expanded);
        say(b"CMD ECHO ");
        say(&expanded[..length]);
        say(b"\r\n");
    } else if name.starts_with(b"DIR") {
        directory(rest);
    } else if name.starts_with(b"TYPE") {
        show_file(rest);
    } else if name.starts_with(b"COPY") {
        let (from, to) = split(rest);
        copy(from, to);
    } else if name.starts_with(b"DEL") {
        one(rest, disk::remove_file, b"CMD DEL OK\r\n", b"CMD DEL FAIL\r\n");
    } else if name.starts_with(b"REN") {
        let (from, to) = split(rest);
        two(from, to, disk::rename_file, b"CMD REN OK\r\n", b"CMD REN FAIL\r\n");
    } else if name.starts_with(b"MD") {
        one(rest, disk::make_dir, b"CMD MD OK\r\n", b"CMD MD FAIL\r\n");
    } else if name.starts_with(b"CD") {
        if disk::change_dir(rest) {
            say(b"CMD CD OK\r\n");
        } else {
            say(b"CMD CD FAIL\r\n");
        }
    } else if name.starts_with(b"RD") {
        one(rest, disk::remove_dir, b"CMD RD OK\r\n", b"CMD RD FAIL\r\n");
    } else if name.starts_with(b"SET") {
        assign(rest);
    } else if let Some(file) = disk::canonical(verb) {
        launch_named(&file, rest, ram, session, depth);
    }
}

fn directory(rest: &[u8]) {
    let Some(name) = disk::canonical(rest) else {
        say(b"CMD DIR FAIL\r\n");
        return;
    };
    if disk::exists(&name) {
        say(b"CMD DIR ");
        say(rest);
        say(b"\r\n");
    } else {
        say(b"CMD DIR FAIL\r\n");
    }
}

fn show_file(rest: &[u8]) {
    let Some(name) = disk::canonical(rest) else {
        say(b"CMD TYPE FAIL\r\n");
        return;
    };
    let mut body = [0u8; 64];
    let Some(length) = disk::read_file(&name, &mut body) else {
        say(b"CMD TYPE FAIL\r\n");
        return;
    };
    say(b"CMD TYPE ");
    say(&body[..length]);
    say(b"\r\n");
}

fn copy(from: &[u8], to: &[u8]) {
    let (Some(source), Some(dest)) = (disk::canonical(from), disk::canonical(to)) else {
        say(b"CMD COPY FAIL\r\n");
        return;
    };
    let mut body = [0u8; 128];
    let Some(length) = disk::read_file(&source, &mut body) else {
        say(b"CMD COPY FAIL\r\n");
        return;
    };
    if disk::write_file(&dest, &body[..length]) {
        say(b"CMD COPY OK\r\n");
    } else {
        say(b"CMD COPY FAIL\r\n");
    }
}

fn one(rest: &[u8], act: fn(&[u8; 11]) -> bool, ok: &[u8], fail: &[u8]) {
    let Some(name) = disk::canonical(rest) else {
        say(fail);
        return;
    };
    if act(&name) { say(ok) } else { say(fail) }
}

fn two(from: &[u8], to: &[u8], act: fn(&[u8; 11], &[u8; 11]) -> bool, ok: &[u8], fail: &[u8]) {
    let (Some(source), Some(dest)) = (disk::canonical(from), disk::canonical(to)) else {
        say(fail);
        return;
    };
    if act(&source, &dest) { say(ok) } else { say(fail) }
}

fn assign(rest: &[u8]) {
    let mut split_at = None;
    let mut index = 0usize;
    while index < rest.len() {
        if rest[index] == b'=' {
            split_at = Some(index);
            break;
        }
        index += 1;
    }
    let Some(at) = split_at else {
        say(b"CMD SET FAIL\r\n");
        return;
    };
    let mut key = [0u8; 8];
    let count = at.min(key.len());
    key[..count].copy_from_slice(&rest[..count]);
    upper(&mut key[..count]);
    let value = &rest[at + 1..];
    let env = env();
    let mut slot = 0usize;
    while slot < env.len() {
        if !env[slot].live || env[slot].name == key {
            env[slot].name = key;
            env[slot].value_len = value.len().min(env[slot].value.len());
            env[slot].value[..env[slot].value_len].copy_from_slice(&value[..env[slot].value_len]);
            env[slot].live = true;
            say(b"CMD SET OK\r\n");
            return;
        }
        slot += 1;
    }
    say(b"CMD SET FAIL\r\n");
}

fn launch_named(
    name: &[u8; 11],
    args: &[u8],
    ram: &mut GuestRam,
    session: &mut Session,
    depth: u8,
) {
    if &name[8..11] == b"BAT" {
        let mut body = [0u8; 512];
        let Some(length) = disk::read_file(name, &mut body) else {
            say(b"CMD BAT FAIL\r\n");
            return;
        };
        run_batch(&body[..length], ram, session, depth + 1);
        return;
    }
    let mut image = [0u8; 32768];
    let Some(length) = disk::read_file(name, &mut image) else {
        say(b"CMD RUN FAIL\r\n");
        return;
    };
    let code = super::run::spawn(&image[..length], args, ram, session);
    if code != 0 {
        say(b"CMD ERRORLEVEL ");
        say(&[code.wrapping_add(b'0')]);
        say(b"\r\n");
    }
}

fn run_batch(body: &[u8], ram: &mut GuestRam, session: &mut Session, depth: u8) {
    let mut start = 0usize;
    let mut index = 0usize;
    while index <= body.len() {
        let end = index == body.len() || body[index] == b'\r' || body[index] == b'\n';
        if end && index > start {
            execute(&body[start..index], ram, session, depth);
        }
        if index < body.len() && (body[index] == b'\r' || body[index] == b'\n') {
            start = index + 1;
        }
        index += 1;
    }
}

fn expand(text: &[u8], out: &mut [u8]) -> usize {
    let mut written = 0usize;
    let mut index = 0usize;
    while index < text.len() && written < out.len() {
        if text[index] == b'%' {
            let mut end = index + 1;
            while end < text.len() && text[end] != b'%' {
                end += 1;
            }
            if end < text.len() {
                if let Some(value) = lookup(&text[index + 1..end]) {
                    let mut copy = 0usize;
                    while copy < value.len() && written < out.len() {
                        out[written] = value[copy];
                        written += 1;
                        copy += 1;
                    }
                }
                index = end + 1;
                continue;
            }
        }
        out[written] = text[index];
        written += 1;
        index += 1;
    }
    written
}

fn lookup(name: &[u8]) -> Option<&'static [u8]> {
    let mut key = [0u8; 8];
    let count = name.len().min(key.len());
    key[..count].copy_from_slice(&name[..count]);
    upper(&mut key[..count]);
    let env = env();
    let mut index = 0usize;
    while index < env.len() {
        if env[index].live && env[index].name == key {
            return Some(&env[index].value[..env[index].value_len]);
        }
        index += 1;
    }
    None
}

fn trim(text: &[u8]) -> &[u8] {
    let mut start = 0usize;
    while start < text.len() && (text[start] == b' ' || text[start] == b'\t') {
        start += 1;
    }
    let mut end = text.len();
    while end > start && (text[end - 1] == b' ' || text[end - 1] == b'\r') {
        end -= 1;
    }
    &text[start..end]
}

fn split(text: &[u8]) -> (&[u8], &[u8]) {
    let mut index = 0usize;
    while index < text.len() && text[index] != b' ' {
        index += 1;
    }
    let verb = &text[..index];
    let mut rest = index;
    while rest < text.len() && text[rest] == b' ' {
        rest += 1;
    }
    (verb, &text[rest..])
}

fn upper(text: &mut [u8]) {
    let mut index = 0usize;
    while index < text.len() {
        if text[index].is_ascii_lowercase() {
            text[index] = text[index].wrapping_sub(b'a').wrapping_add(b'A');
        }
        index += 1;
    }
}

fn say(text: &[u8]) {
    crate::bios::mscon::con_write_bytes(text);
}

fn env() -> &'static mut [Binding; ROWS] {
    unsafe { &mut *core::ptr::addr_of_mut!(ENV) }
}
