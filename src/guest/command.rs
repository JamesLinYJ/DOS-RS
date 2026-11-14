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

pub(super) struct Environment([Binding; ROWS]);

impl Environment {
    pub(super) fn new() -> Self {
        Self(
            [Binding {
                name: [0; 8],
                value: [0; 16],
                value_len: 0,
                live: false,
            }; ROWS],
        )
    }

    fn key(name: &[u8]) -> [u8; 8] {
        let mut key = [0; 8];
        let n = name.len().min(key.len());
        key[..n].copy_from_slice(&name[..n]);
        key.make_ascii_uppercase();
        key
    }

    fn set(&mut self, name: &[u8], value: &[u8]) -> bool {
        let key = Self::key(name);
        let index = self
            .0
            .iter()
            .position(|b| b.live && b.name == key)
            .or_else(|| self.0.iter().position(|b| !b.live));
        let Some(index) = index else { return false };
        let entry = &mut self.0[index];
        entry.name = key;
        entry.value_len = value.len().min(entry.value.len());
        entry.value[..entry.value_len].copy_from_slice(&value[..entry.value_len]);
        entry.live = true;
        true
    }

    fn value(&self, name: &[u8]) -> Option<&[u8]> {
        let key = Self::key(name);
        let entry = self.0.iter().find(|b| b.live && b.name == key)?;
        Some(&entry.value[..entry.value_len])
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
    execute(&line[..length], ram, session, 0, true);
}

pub(super) fn execute(
    line: &[u8],
    ram: &mut GuestRam,
    session: &mut Session,
    depth: u8,
    echo: bool,
) {
    let line = line.trim_ascii();
    if line.is_empty() || depth > 4 {
        return;
    }
    let (verb, rest) = split(line);
    if crate::drivers::command(verb, rest) {
        return;
    }
    if verb.eq_ignore_ascii_case(b"HW") {
        crate::platform::report();
    } else if verb.eq_ignore_ascii_case(b"COLOR") {
        let value = core::str::from_utf8(rest)
            .ok()
            .and_then(|s| u8::from_str_radix(s, 16).ok());
        if rest.is_empty() {
            crate::bios::pc::recolor(7);
        } else if let Some(v) = value
            && rest.len() == 2
            && v >> 4 != v & 15
        {
            crate::bios::pc::recolor(v);
        } else {
            say(b"COLOR <background><foreground> (hex digits)\r\n");
        }
    } else if verb.eq_ignore_ascii_case(b"VER") {
        say(b"DOS-RS (DOS 4.00)\r\n");
    } else if verb.eq_ignore_ascii_case(b"ECHO") {
        let mut expanded = [0; 80];
        let length = expand(rest, &mut expanded, &session.environment);
        say(&expanded[..length]);
        say(b"\r\n");
    } else if verb.eq_ignore_ascii_case(b"DIR") {
        directory(rest);
    } else if verb.eq_ignore_ascii_case(b"TYPE") {
        show_file(rest);
    } else if verb.eq_ignore_ascii_case(b"COPY") {
        let (from, to) = split(rest);
        copy(from, to);
    } else if verb.eq_ignore_ascii_case(b"DEL") {
        one(
            rest,
            disk::remove_file,
            b"File deleted.\r\n",
            b"Delete failed.\r\n",
        );
    } else if verb.eq_ignore_ascii_case(b"REN") {
        let (from, to) = split(rest);
        two(
            from,
            to,
            disk::rename_file,
            b"File renamed.\r\n",
            b"Rename failed.\r\n",
        );
    } else if verb.eq_ignore_ascii_case(b"MD") {
        one(
            rest,
            disk::make_dir,
            b"Directory created.\r\n",
            b"Cannot create directory.\r\n",
        );
    } else if verb.eq_ignore_ascii_case(b"CD") {
        if disk::change_dir(rest) {
            say(b"Directory changed.\r\n");
        } else {
            say(b"Directory not found.\r\n");
        }
    } else if verb.eq_ignore_ascii_case(b"RD") {
        one(
            rest,
            disk::remove_dir,
            b"Directory removed.\r\n",
            b"Cannot remove directory.\r\n",
        );
    } else if verb.eq_ignore_ascii_case(b"SET") {
        assign(rest, &mut session.environment);
    } else if let Some(mut file) = disk::canonical(verb) {
        if !verb.contains(&b'.') {
            for ext in [b"COM", b"EXE", b"BAT"] {
                file[8..].copy_from_slice(ext);
                if disk::exists(&file) {
                    break;
                }
            }
            if verb.eq_ignore_ascii_case(b"MEM") && !disk::exists(&file) {
                if rest.is_empty() {
                    let (largest, total) = ram.xms_free_bytes();
                    say(b"XMS and EMS share this terminal's extended memory.\r\n");
                    say(b"Free XMS:     ");
                    crate::platform::number((total / 1024) as u32);
                    say(b" KiB\r\nLargest XMS:  ");
                    crate::platform::number((largest / 1024) as u32);
                    say(b" KiB\r\nFree EMS:     ");
                    crate::platform::number(ram.free_ems() as u32);
                    say(b" pages of 16 KiB\r\n");
                } else {
                    say(b"Use MEM for a summary or install FreeDOS MEM for switches.\r\n");
                }
                return;
            }
        }
        if echo {
            let mut path = [0; 64];
            let len = disk::current_path(&mut path);
            say(b"A:\\");
            say(&path[..len]);
            say(b"> ");
            say(line);
            say(b"\r\n");
        }
        launch_named(&file, rest, ram, session, depth);
    }
}

fn directory(rest: &[u8]) {
    let Some(name) = disk::canonical(rest) else {
        say(b"File not found.\r\n");
        return;
    };
    if disk::exists(&name) {
        say(b"Found: ");
        say(rest);
        say(b"\r\n");
    } else {
        say(b"File not found.\r\n");
    }
}

fn show_file(rest: &[u8]) {
    let Some(name) = disk::canonical(rest) else {
        say(b"Cannot read file.\r\n");
        return;
    };
    let mut body = [0u8; 256];
    let Some(length) = disk::read_head(&name, &mut body) else {
        say(b"Cannot read file.\r\n");
        return;
    };
    say(&body[..length]);
    say(b"\r\n");
}

fn copy(from: &[u8], to: &[u8]) {
    let (Some(source), Some(dest)) = (disk::canonical(from), disk::canonical(to)) else {
        say(b"Copy failed.\r\n");
        return;
    };
    let mut body = [0u8; 128];
    let Some(length) = disk::read_file(&source, &mut body) else {
        say(b"Copy failed.\r\n");
        return;
    };
    if disk::write_file(&dest, &body[..length]) {
        say(b"File copied.\r\n");
    } else {
        say(b"Copy failed.\r\n");
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
    if act(&source, &dest) {
        say(ok)
    } else {
        say(fail)
    }
}

fn assign(rest: &[u8], environment: &mut Environment) {
    let Some(separator) = rest.iter().position(|&byte| byte == b'=') else {
        say(b"Usage: SET name=value\r\n");
        return;
    };
    if environment.set(&rest[..separator], &rest[separator + 1..]) {
        say(b"Variable set.\r\n");
    } else {
        say(b"Environment is full.\r\n");
    }
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
            say(b"Cannot read batch file.\r\n");
            return;
        };
        if session.scheduled {
            if !session.batches.push(body, length, session.child_depth) {
                say(b"Batch nesting limit reached.\r\n");
            }
        } else {
            run_batch(&body[..length], ram, session, depth + 1);
        }
        return;
    }
    let Some(code) = super::run::spawn(args, name, ram, session) else {
        say(b"Cannot load program.\r\n");
        return;
    };
    if code != 0 {
        say(b"Exit code: ");
        crate::bios::mscon::write_number(code);
        say(b"\r\n");
    }
}

fn run_batch(body: &[u8], ram: &mut GuestRam, session: &mut Session, depth: u8) {
    for line in body.split(|&byte| matches!(byte, b'\r' | b'\n')) {
        execute(line, ram, session, depth, true);
    }
}

fn expand(text: &[u8], out: &mut [u8], env: &Environment) -> usize {
    let mut written = 0usize;
    let mut index = 0usize;
    while index < text.len() && written < out.len() {
        if text[index] == b'%' {
            let mut end = index + 1;
            while end < text.len() && text[end] != b'%' {
                end += 1;
            }
            if end < text.len() {
                if let Some(value) = env.value(&text[index + 1..end]) {
                    let n = value.len().min(out.len() - written);
                    out[written..written + n].copy_from_slice(&value[..n]);
                    written += n;
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

fn split(text: &[u8]) -> (&[u8], &[u8]) {
    let end = text
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(text.len());
    (&text[..end], text[end..].trim_ascii_start())
}

fn say(text: &[u8]) {
    crate::bios::mscon::con_write_bytes(text);
}

struct Batch {
    body: [u8; 512],
    end: usize,
    pos: usize,
    owner: u8,
}
pub(super) struct Batches {
    stack: [Option<Batch>; 5],
    depth: usize,
}
impl Batches {
    pub(super) fn new() -> Self {
        Self {
            stack: [const { None }; 5],
            depth: 0,
        }
    }
    fn push(&mut self, body: [u8; 512], end: usize, owner: u8) -> bool {
        if self.depth == self.stack.len() {
            return false;
        }
        self.stack[self.depth] = Some(Batch {
            body,
            end,
            pos: 0,
            owner,
        });
        self.depth += 1;
        true
    }
    pub(super) fn ready(&self, owner: u8) -> bool {
        self.depth > 0
            && self.stack[self.depth - 1]
                .as_ref()
                .is_some_and(|b| b.owner == owner)
    }
    pub(super) fn line(&mut self, owner: u8, out: &mut [u8; 512]) -> Option<usize> {
        if !self.ready(owner) {
            return None;
        }
        let top = self.depth - 1;
        let b = self.stack[top].as_mut()?;
        let line = &b.body[b.pos..b.end];
        let n = line
            .iter()
            .position(|v| matches!(v, b'\n' | b'\r'))
            .unwrap_or(line.len());
        out[..n].copy_from_slice(&line[..n]);
        b.pos += n;
        while b.pos < b.end && matches!(b.body[b.pos], b'\n' | b'\r') {
            b.pos += 1;
        }
        if b.pos == b.end {
            self.stack[top] = None;
            self.depth -= 1;
        }
        Some(n)
    }
}
