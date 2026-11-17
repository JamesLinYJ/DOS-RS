//! Word expansions retain quote and substitution boundaries until IFS splitting
//! and pathname matching have finished. Case and parameter patterns share these rules.
use super::{
    Shell, diagnostic,
    syntax::{self, Modifier, Part, Word},
};
use alloc::{
    ffi::CString,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::ffi::{CStr, c_char, c_void};

unsafe extern "C" {
    fn dosrs_shell_home(login: *const c_char) -> *const c_char;
    fn fnmatch(pattern: *const c_char, subject: *const c_char, flags: i32) -> i32;
    fn dosrs_shell_glob(
        pattern: *const c_char,
        context: *mut c_void,
        append: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    ) -> i32;
}

#[derive(Clone, Copy, PartialEq)]
enum Origin {
    Literal,
    Quoted,
    Expansion,
}
enum Piece {
    Text(String, Origin),
    Boundary,
}

fn value(text: String, quoted: bool) -> Piece {
    Piece::Text(
        text,
        if quoted {
            Origin::Quoted
        } else {
            Origin::Expansion
        },
    )
}

#[derive(Default)]
struct Field {
    text: String,
    pattern: String,
    present: bool,
    wildcard: bool,
}

impl Field {
    fn push(&mut self, ch: char, quoted: bool) {
        self.text.push(ch);
        if quoted && "\\*?[]-!^".contains(ch) {
            self.pattern.push('\\');
        }
        self.pattern.push(ch);
        self.wildcard |= !quoted && "*?[".contains(ch);
        self.present = true;
    }

    fn finish(&mut self, fields: &mut Vec<String>, glob: bool) -> Result<(), ()> {
        if glob && self.wildcard {
            let pattern = CString::new(self.pattern.as_str())
                .map_err(|_| diagnostic("invalid pathname pattern"))?;
            // The C adapter owns glob_t. It calls append synchronously with live,
            // NUL-terminated strings and frees the vector before returning.
            unsafe extern "C" fn append(ctx: *mut c_void, path: *const c_char) -> i32 {
                let fields = unsafe { &mut *ctx.cast::<Vec<String>>() };
                let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
                    return -1;
                };
                fields.push(path.into());
                0
            }
            let status = unsafe {
                dosrs_shell_glob(
                    pattern.as_ptr(),
                    (fields as *mut Vec<String>).cast(),
                    append,
                )
            };
            match status {
                0 => {
                    *self = Self::default();
                    return Ok(());
                }
                1 => (), // No matching filenames: keep the unexpanded word.
                _ => {
                    diagnostic("pathname expansion failed");
                    return Err(());
                }
            }
        }
        fields.push(core::mem::take(&mut self.text));
        *self = Self::default();
        Ok(())
    }
}

fn scalar(pieces: Vec<Piece>) -> String {
    let mut s = String::new();
    for piece in pieces {
        if let Piece::Text(text, _) = piece {
            s.push_str(&text);
        }
    }
    s
}

impl Shell {
    pub(super) fn ifs(&self) -> &str {
        self.vars.get("IFS").map_or(" \t\n", |v| v.value.as_str())
    }

    fn positional_value(&self) -> String {
        let sep = self
            .ifs()
            .chars()
            .next()
            .map_or_else(String::new, |c| c.to_string());
        self.args.get(1..).unwrap_or_default().join(&sep)
    }

    fn parameter_value(&self, name: &str) -> Option<String> {
        match name {
            "?" => Some(self.last.to_string()),
            "$" => Some(self.shell_pid.to_string()),
            "#" => Some(self.args.len().saturating_sub(1).to_string()),
            "!" => self
                .jobs
                .last()
                .map(super::jobs::Job::last_pid)
                .or_else(|| self.background.last().copied())
                .map(|p| p.to_string()),
            "-" => Some(self.option_flags()),
            "@" | "*" => (self.args.len() > 1).then(|| self.positional_value()),
            _ if name.as_bytes()[0].is_ascii_digit() => name
                .parse::<usize>()
                .ok()
                .and_then(|i| self.args.get(i).cloned()),
            _ => self.vars.get(name).map(|v| v.value.clone()),
        }
    }

    fn expansion_error(&mut self, msg: &str) {
        diagnostic(msg);
        if !self.interactive {
            self.exiting = true;
        }
    }

    fn add_parameter(
        &mut self,
        pieces: &mut Vec<Piece>,
        name: &str,
        quoted: bool,
        multiple: bool,
    ) -> Result<(), ()> {
        if matches!(name, "@" | "*") && multiple && (name == "@" || !quoted) {
            for (i, arg) in self.args.iter().skip(1).enumerate() {
                if i != 0 {
                    pieces.push(Piece::Boundary);
                }
                pieces.push(value(arg.clone(), quoted));
            }
        } else {
            let text = self.parameter_value(name);
            if self.nounset && text.is_none() && !matches!(name, "@" | "*") {
                self.expansion_error(&format!("{name}: parameter is unset"));
                return Err(());
            }
            pieces.push(value(text.unwrap_or_default(), quoted));
        }
        Ok(())
    }

    fn home(&self, login: &str) -> Option<String> {
        if login.is_empty()
            && let Some(home) = self.vars.get("HOME")
        {
            return Some(home.value.clone());
        }
        let name = CString::new(login).ok()?;
        let ptr = unsafe {
            dosrs_shell_home(if login.is_empty() {
                core::ptr::null()
            } else {
                name.as_ptr()
            })
        };
        if ptr.is_null() {
            return None;
        }
        // musl owns the passwd record until the next account lookup. Copy it
        // while the single-threaded shell is still evaluating this prefix.
        unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .ok()
            .map(String::from)
    }

    fn literal_tildes(
        &self,
        out: &mut Vec<Piece>,
        text: &str,
        index: usize,
        last: bool,
        assignment: bool,
    ) {
        let first = if index == 0 && assignment {
            text.find('=').map(|n| n + 1)
        } else {
            (index == 0).then_some(0)
        };
        let mut start = 0;
        for (at, _) in text.match_indices('~') {
            if first != Some(at) && !(assignment && text[..at].ends_with(':')) {
                continue;
            }
            let name_start = at + 1;
            let end = text[name_start..]
                .find(|c| c == '/' || (assignment && c == ':'))
                .map_or(text.len(), |n| name_start + n);
            if end == text.len() && !last {
                continue;
            }
            let Some(home) = self.home(&text[name_start..end]) else {
                continue;
            };
            if start < at {
                out.push(Piece::Text(text[start..at].into(), Origin::Literal));
            }
            out.push(Piece::Text(home, Origin::Quoted));
            start = end;
        }
        if start < text.len() {
            out.push(Piece::Text(text[start..].into(), Origin::Literal));
        }
    }

    pub(super) fn assignment(&mut self, word: &Word) -> Result<String, ()> {
        Ok(scalar(self.word_pieces(word, false, true)?))
    }

    fn pieces(&mut self, word: &Word, multiple: bool) -> Result<Vec<Piece>, ()> {
        self.word_pieces(word, multiple, false)
    }

    fn word_pieces(
        &mut self,
        word: &Word,
        multiple: bool,
        assignment: bool,
    ) -> Result<Vec<Piece>, ()> {
        let mut out = Vec::new();
        for (i, part) in word.iter().enumerate() {
            match part {
                Part::Literal(s) => {
                    self.literal_tildes(&mut out, s, i, i + 1 == word.len(), assignment)
                }
                Part::Quoted(s) => out.push(Piece::Text(s.clone(), Origin::Quoted)),
                Part::Parameter(name, q) => self.add_parameter(&mut out, name, *q, multiple)?,
                Part::Substitution(body, q) => out.push(value(self.capture(body)?, *q)),
                Part::Arithmetic(word, q) => out.push(value(self.arithmetic(word)?, *q)),
                Part::Braced(name, modifier, q) => {
                    self.modified(&mut out, name, modifier, *q, multiple)?
                }
            }
        }
        Ok(out)
    }

    fn modified(
        &mut self,
        out: &mut Vec<Piece>,
        name: &str,
        modifier: &Modifier,
        quoted: bool,
        multiple: bool,
    ) -> Result<(), ()> {
        let current = self.parameter_value(name);
        if let Modifier::Select { op, colon, word } = modifier {
            let missing = current.is_none() || (*colon && current.as_deref() == Some(""));
            if missing == (*op == '+') {
                if *op == '+' {
                    out.push(value(String::new(), quoted));
                } else {
                    self.add_parameter(out, name, quoted, multiple)?;
                }
                return Ok(());
            }
            let mut replacement = self.pieces(word, multiple && matches!(op, '-' | '+'))?;
            match *op {
                '?' => {
                    let text = scalar(replacement);
                    self.expansion_error(&format!(
                        "{name}: {}",
                        if text.is_empty() {
                            "parameter is unset or empty"
                        } else {
                            &text
                        }
                    ));
                    return Err(());
                }
                '=' => {
                    if !syntax::identifier(name) {
                        self.expansion_error("cannot assign to a positional or special parameter");
                        return Err(());
                    }
                    let text = scalar(replacement);
                    self.set(name, &text);
                    out.push(value(text, quoted));
                }
                _ => {
                    if replacement.is_empty() && word.is_empty() {
                        out.push(value(String::new(), quoted));
                    }
                    for piece in &mut replacement {
                        if let Piece::Text(_, origin) = piece {
                            if quoted {
                                *origin = Origin::Quoted;
                            } else if *origin == Origin::Literal {
                                *origin = Origin::Expansion;
                            }
                        }
                    }
                    out.extend(replacement);
                }
            }
            return Ok(());
        }
        if self.nounset && current.is_none() && !matches!(name, "@" | "*") {
            self.expansion_error(&format!("{name}: parameter is unset"));
            return Err(());
        }
        let text = current.unwrap_or_default();
        let result = match modifier {
            Modifier::Length => text.chars().count().to_string(),
            Modifier::Trim {
                prefix,
                longest,
                pattern,
            } => {
                let pattern = self.pattern(pattern)?;
                trim(&text, &pattern, *prefix, *longest).to_string()
            }
            Modifier::Select { .. } => unreachable!(),
        };
        out.push(value(result, quoted));
        Ok(())
    }

    pub(super) fn pattern(&mut self, word: &Word) -> Result<CString, ()> {
        let mut field = Field::default();
        for piece in self.pieces(word, false)? {
            if let Piece::Text(text, origin) = piece {
                for c in text.chars() {
                    field.push(c, origin == Origin::Quoted);
                }
            }
        }
        CString::new(field.pattern).map_err(|_| diagnostic("invalid pattern"))
    }

    pub(super) fn expand(&mut self, word: &Word, split: bool) -> Result<Vec<String>, ()> {
        let pieces = self.pieces(word, split)?;
        if !split {
            return Ok(alloc::vec![scalar(pieces)]);
        }
        let mut field = Field::default();
        let mut fields = Vec::new();
        let mut whitespace = false;
        let glob = !self.noglob;
        let ifs = self.ifs();
        for piece in pieces {
            let Piece::Text(text, origin) = piece else {
                if field.present {
                    field.finish(&mut fields, glob)?;
                }
                whitespace = false;
                continue;
            };
            if text.is_empty() && origin == Origin::Quoted {
                if whitespace && field.present {
                    field.finish(&mut fields, glob)?;
                }
                whitespace = false;
                field.present = true;
            }
            for c in text.chars() {
                if origin == Origin::Expansion && ifs.contains(c) {
                    if matches!(c, ' ' | '\t' | '\n') {
                        whitespace = true;
                    } else {
                        field.finish(&mut fields, glob)?;
                        whitespace = false;
                    }
                } else {
                    if whitespace && field.present {
                        field.finish(&mut fields, glob)?;
                    }
                    whitespace = false;
                    field.push(c, origin == Origin::Quoted);
                }
            }
        }
        if field.present {
            field.finish(&mut fields, glob)?;
        }
        Ok(fields)
    }
}

pub(super) fn matches(pattern: &CStr, subject: &CStr) -> bool {
    unsafe { fnmatch(pattern.as_ptr(), subject.as_ptr(), 0) == 0 }
}

fn trim<'a>(text: &'a str, pattern: &CStr, prefix: bool, longest: bool) -> &'a str {
    let boundaries = text
        .char_indices()
        .map(|(at, _)| at)
        .chain(core::iter::once(text.len()));
    let mut test = |at| {
        let (left, right) = text.split_at(at);
        let (candidate, remainder) = if prefix { (left, right) } else { (right, left) };
        CString::new(candidate)
            .is_ok_and(|s| matches(pattern, &s))
            .then_some(remainder)
    };
    if prefix == longest {
        boundaries.rev().find_map(&mut test)
    } else {
        boundaries.into_iter().find_map(&mut test)
    }
    .unwrap_or(text)
}
