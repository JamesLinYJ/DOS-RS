//! Bounded native COMMAND environment. Names and values are DOS byte strings.

use super::envdata::{COMSPEC_KEY, ECOMSPEC};
use crate::bios::mscon::con_write_bytes as print;

#[derive(Clone, Copy)]
struct Text<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Text<N> {
    fn from_slice(input: &[u8]) -> Self {
        let mut bytes = [0; N];
        let len = input.len().min(N);
        bytes[..len].copy_from_slice(&input[..len]);
        Self { bytes, len }
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[derive(Clone, Copy)]
struct Variable {
    name: Text<16>,
    value: Text<32>,
}

pub struct Environment {
    vars: [Option<Variable>; 4],
    path: Text<67>,
    prompt: Text<64>,
    pub(super) echo: bool,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            vars: [None; 4],
            path: Text::from_slice(b""),
            prompt: Text::from_slice(b""),
            echo: true,
        }
    }

    pub(super) fn prompt_cmd(&mut self, tail: &[u8]) {
        self.prompt = Text::from_slice(argument(tail));
        let mut bytes = self.prompt.bytes().iter().copied();
        if bytes.len() == 0 {
            return;
        }
        while let Some(byte) = bytes.next() {
            if byte != b'$' {
                print(&[byte]);
                continue;
            }
            match bytes.next().map(|b| b.to_ascii_uppercase()) {
                Some(b'P') => print(super::tcmd1a::cwd_text()),
                Some(b'G') => print(b">"),
                Some(b'$') | None => print(b"$"),
                _ => {}
            }
        }
        print(b"\r\n");
    }

    pub(super) fn path_cmd(&mut self, tail: &[u8]) {
        let mut args = argument(tail)
            .split(|b| matches!(b, b' ' | b'\t'))
            .filter(|s| !s.is_empty());
        match (args.next(), args.next()) {
            (None, _) if self.path.bytes().is_empty() => print(b"No Path\r\n"),
            (None, _) => self.print_path(),
            (Some(path), None) => self.path = Text::from_slice(path),
            _ => print(b"Too many parameters\r\n"),
        }
    }

    fn print_path(&self) {
        print(b"PATH=");
        print(self.path.bytes());
        print(b"\r\n");
    }

    pub(super) fn set_cmd(&mut self, tail: &[u8]) {
        let line = argument(tail);
        if line.is_empty() {
            self.print_path();
            print(COMSPEC_KEY);
            print(ECOMSPEC);
            print(b"\r\n");
            for var in self.vars.iter().flatten() {
                print(var.name.bytes());
                print(b"=");
                print(var.value.bytes());
                print(b"\r\n");
            }
            return;
        }

        let mut fields = line.split(|&byte| byte == b'=');
        let (Some(name), Some(value), None) = (fields.next(), fields.next(), fields.next()) else {
            print(b"Syntax error\r\n");
            return;
        };
        if name.is_empty() {
            print(b"Syntax error\r\n");
            return;
        }
        // A delete compares the full supplied name. New assignments store up to 16 bytes.
        let key = if value.is_empty() {
            name
        } else {
            &name[..name.len().min(16)]
        };
        let existing = self
            .vars
            .iter()
            .position(|slot| slot.is_some_and(|v| v.name.bytes().eq_ignore_ascii_case(key)));
        if value.is_empty() {
            if let Some(i) = existing {
                self.vars[i] = None;
            }
            return;
        }
        let Some(i) = existing.or_else(|| self.vars.iter().position(Option::is_none)) else {
            print(b"Out of environment space\r\n");
            return;
        };
        let mut name = Text::from_slice(key);
        name.bytes.make_ascii_uppercase();
        self.vars[i] = Some(Variable {
            name,
            value: Text::from_slice(value),
        });
    }
}

fn argument(tail: &[u8]) -> &[u8] {
    let end = tail
        .iter()
        .position(|b| matches!(b, b'\r' | b'\n' | 0))
        .unwrap_or(tail.len());
    let text = &tail[..end];
    let start = text
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t'))
        .unwrap_or(text.len());
    &text[start..]
}
