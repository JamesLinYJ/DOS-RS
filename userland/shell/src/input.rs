//! The read utility consumes exactly one record so subsequent readers see
//! the remaining bytes even on pipes. Only unescaped IFS bytes split fields.
use super::{Shell, diagnostic, read, syntax, write_all};
use alloc::{format, string::String, vec::Vec};
use core::ffi::c_int;

unsafe extern "C" {
    fn isatty(fd: c_int) -> c_int;
}

fn options(args: &[String]) -> Result<(bool, u8, &[String]), ()> {
    let mut raw = false;
    let mut delimiter = b'\n';
    let mut i = 0;
    while let Some(option) = args.get(i) {
        if option == "--" {
            i += 1;
            break;
        }
        let Some(flags) = option.strip_prefix('-').filter(|s| !s.is_empty()) else {
            break;
        };
        i += 1;
        let mut flags = flags.chars();
        while let Some(ch) = flags.next() {
            match ch {
                'r' => raw = true,
                'd' => {
                    let text = if !flags.as_str().is_empty() {
                        flags.as_str()
                    } else {
                        let value = args
                            .get(i)
                            .ok_or_else(|| diagnostic("read: -d requires a delimiter"))?;
                        i += 1;
                        value.as_str()
                    };
                    delimiter = text.as_bytes().first().copied().unwrap_or(0);
                    break;
                }
                _ => {
                    diagnostic(&format!("read: unknown option {ch}"));
                    return Err(());
                }
            }
        }
    }
    if i == args.len() {
        diagnostic("read: expected a variable name");
        return Err(());
    }
    Ok((raw, delimiter, &args[i..]))
}

struct Character {
    ch: char,
    quoted: bool,
}

fn record(raw: bool, delimiter: u8, prompt: Option<&str>) -> Result<(Vec<Character>, bool), ()> {
    let mut bytes = Vec::new();
    let mut quoted = Vec::new();
    let mut escape = false;
    let mut b = [0];
    let complete = loop {
        match read(0, &mut b) {
            Ok(0) => break false,
            Err(4) => continue,
            Err(e) => {
                diagnostic(&format!("read: input error {e}"));
                return Err(());
            }
            Ok(_) => (),
        }
        if !escape && b[0] == delimiter {
            break true;
        }
        if escape && b[0] == b'\n' {
            if let Some(text) = prompt {
                let _ = write_all(2, text.as_bytes());
            }
            escape = false;
            continue;
        }
        if !raw && !escape && b[0] == b'\\' {
            escape = true;
            continue;
        }
        bytes.push(b[0]);
        quoted.push(escape);
        escape = false;
    };
    let text =
        core::str::from_utf8(&bytes).map_err(|_| diagnostic("read: invalid input encoding"))?;
    Ok((
        text.char_indices()
            .map(|(at, ch)| Character {
                ch,
                quoted: quoted[at],
            })
            .collect(),
        complete,
    ))
}

fn variables(line: &[Character], ifs: &str, count: usize) -> Vec<String> {
    let sep = |c: &Character| !c.quoted && ifs.contains(c.ch);
    let white = |c: &Character| sep(c) && matches!(c.ch, ' ' | '\t' | '\n');
    let strip = |mut at: usize| {
        while line.get(at).is_some_and(white) {
            at += 1;
        }
        at
    };
    let text = |slice: &[Character]| slice.iter().map(|c| c.ch).collect();
    let mut at = strip(0);
    let mut values = Vec::with_capacity(count);
    for _ in 1..count {
        let start = at;
        while line.get(at).is_some_and(|c| !sep(c)) {
            at += 1;
        }
        values.push(text(&line[start..at]));
        at = strip(at);
        if line.get(at).is_some_and(sep) {
            at = strip(at + 1);
        }
    }
    let mut end = line.len();
    while end > at && white(&line[end - 1]) {
        end -= 1;
    }
    values.push(text(&line[at..end]));
    values
}

impl Shell {
    pub(super) fn read_variables(&mut self, args: &[String]) -> i32 {
        let Ok((raw, delimiter, names)) = options(args) else {
            return 2;
        };
        let prompt = if self.interactive && unsafe { isatty(0) } != 0 {
            Some(self.vars.get("PS2").map_or("> ", |v| v.value.as_str()))
        } else {
            None
        };
        let Ok((line, complete)) = record(raw, delimiter, prompt) else {
            return 2;
        };
        for (name, value) in names.iter().zip(variables(&line, self.ifs(), names.len())) {
            if !syntax::identifier(name) {
                diagnostic("read: invalid variable name");
                return 2;
            }
            self.set(name, &value);
        }
        i32::from(!complete)
    }
}
