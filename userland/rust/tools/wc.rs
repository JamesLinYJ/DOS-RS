#![no_std]

use core::ffi::CStr;
use dosrs_user::{
    Args,
    cli::{self, Arg, Output},
    read,
    stat::Stat,
};

#[derive(Default, Clone, Copy)]
struct Counts {
    lines: u64,
    words: u64,
    chars: u64,
    bytes: u64,
    longest: u64,
}
impl Counts {
    fn scan(fd: i32, utf8: bool, gnu_space: bool) -> Result<Self, i32> {
        unsafe extern "C" {
            fn wcwidth(c: i32) -> i32;
        }
        let mut result = Self::default();
        let mut data = [0; 4100]; // four extra bytes can hold an incomplete UTF-8 sequence
        let mut carry = 0;
        let mut in_word = false;
        let mut column = 0;
        loop {
            let count = read(fd, &mut data[carry..carry + 4096])?;
            result.bytes += count as u64;
            let end = carry + count;
            let mut at = 0;
            while at < end {
                let first = data[at];
                let (character, consumed) = if !utf8 || first.is_ascii() {
                    (Some(char::from(first)), 1)
                } else {
                    let length = match first {
                        0xc2..=0xdf => 2,
                        0xe0..=0xef => 3,
                        0xf0..=0xf4 => 4,
                        _ => 1,
                    };
                    if at + length > end && count != 0 {
                        break;
                    }
                    let bytes = &data[at..end.min(at + length)];
                    let c = core::str::from_utf8(bytes)
                        .ok()
                        .and_then(|s| s.chars().next());
                    (c, if c.is_some() { length } else { 1 })
                };
                at += consumed;
                let space = character.is_some_and(|c| {
                    matches!(c, ' ' | '\t'..='\r') ||
                        (utf8 && matches!(c, '\u{1680}' | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200a}' |
                            '\u{2028}' | '\u{2029}' | '\u{205f}' | '\u{3000}')) ||
                        (gnu_space && (c == '\u{a0}' || utf8 && matches!(c, '\u{2007}' | '\u{202f}' | '\u{2060}')))
                });
                if !in_word && !space {
                    result.words += 1;
                }
                in_word = !space;
                if let Some(c) = character {
                    result.chars += 1;
                    match c {
                        '\n' | '\r' | '\x0c' => {
                            result.lines += u64::from(c == '\n');
                            result.longest = result.longest.max(column);
                            column = 0;
                        }
                        '\t' => column += 8 - column % 8,
                        ' '..='~' => column += 1,
                        _ if utf8 => column += unsafe { wcwidth(c as i32) }.max(0) as u64,
                        _ => (),
                    }
                }
            }
            if count == 0 {
                break;
            }
            data.copy_within(at..end, 0);
            carry = end - at;
        }
        result.longest = result.longest.max(column);
        Ok(result)
    }
    fn add(&mut self, v: Self) {
        self.lines += v.lines;
        self.words += v.words;
        self.chars += v.chars;
        self.bytes += v.bytes;
        self.longest = self.longest.max(v.longest);
    }
}

#[derive(PartialEq, Eq, Default)]
enum Total {
    #[default]
    Auto,
    Always,
    Never,
    Only,
}
#[derive(Default)]
struct Options {
    lines: bool,
    words: bool,
    chars: bool,
    bytes: bool,
    longest: bool,
    total: Total,
}
impl Options {
    fn set(&mut self, flag: u8) -> bool {
        match flag {
            b'l' => self.lines = true,
            b'w' => self.words = true,
            b'm' => self.chars = true,
            b'c' => self.bytes = true,
            b'L' => self.longest = true,
            _ => return false,
        }
        true
    }
    fn enabled(&self) -> usize {
        [self.lines, self.words, self.chars, self.bytes, self.longest]
            .into_iter()
            .filter(|&v| v)
            .count()
    }
    fn display(
        &self,
        out: &mut Output,
        v: Counts,
        width: usize,
        name: Option<&[u8]>,
    ) -> Result<(), i32> {
        let mut first = true;
        for (show, n) in [
            (self.lines, v.lines),
            (self.words, v.words),
            (self.chars, v.chars),
            (self.bytes, v.bytes),
            (self.longest, v.longest),
        ] {
            if show {
                if !first {
                    out.push(b" ")?;
                }
                out.number(n, width)?;
                first = false;
            }
        }
        if let Some(n) = name {
            out.push(b" ")?;
            out.push(n)?;
        }
        out.push(b"\n")?;
        out.flush()
    }
}
fn files(args: Args<'_>) -> impl Iterator<Item = Option<&CStr>> {
    let mut names = cli::operands(args).map(Some).peekable();
    let stdin = names.peek().is_none().then_some(None);
    stdin.into_iter().chain(names)
}
fn column_width(args: Args<'_>, opts: &Options, nfiles: usize) -> usize {
    if opts.total == Total::Only || nfiles == 1 && opts.enabled() == 1 {
        return 1;
    }
    let mut total_size = 0u64;
    let mut minimum = 1;
    for name in files(args) {
        if let Ok(metadata) = Stat::file(name) {
            if metadata.mode & 0o170000 == 0o100000 {
                total_size = total_size.saturating_add(metadata.size.max(0) as u64);
            } else {
                minimum = 7;
            }
        }
    }
    minimum.max(total_size.checked_ilog10().unwrap_or(0) as usize + 1)
}
fn run(args: Args<'_>) -> i32 {
    let mut opts = Options::default();
    for arg in cli::arguments(args) {
        match arg {
            Arg::Short(flags) => {
                for flag in flags {
                    if !opts.set(*flag) {
                        return cli::bad_option(b"wc", core::slice::from_ref(flag), false);
                    }
                }
            }
            Arg::Long(name) if name.starts_with(b"total=") => {
                opts.total = match &name[6..] {
                    b"always" => Total::Always,
                    b"auto" => Total::Auto,
                    b"never" => Total::Never,
                    b"only" => Total::Only,
                    _ => return cli::bad_option(b"wc", name, true),
                };
            }
            Arg::Long(name) => {
                let flag = match name {
                    b"lines" => b'l',
                    b"words" => b'w',
                    b"chars" => b'm',
                    b"bytes" => b'c',
                    b"max-line-length" => b'L',
                    _ => return cli::bad_option(b"wc", name, true),
                };
                opts.set(flag);
            }
            Arg::Operand(_) => (),
        }
    }
    if opts.enabled() == 0 {
        opts.lines = true;
        opts.words = true;
        opts.bytes = true;
    }
    let locale = [b"LC_ALL".as_slice(), b"LC_CTYPE", b"LANG"]
        .into_iter()
        .find_map(|key| args.env(key).filter(|v| !v.is_empty()))
        .unwrap_or(b"C");
    let codeset = locale
        .split(|&b| b == b'.')
        .nth(1)
        .unwrap_or(locale)
        .split(|&b| b == b'@')
        .next()
        .unwrap_or(locale);
    let utf8 = codeset.eq_ignore_ascii_case(b"utf-8") || codeset.eq_ignore_ascii_case(b"utf8");
    let gnu_space = args.env(b"POSIXLY_CORRECT").is_none();
    let nfiles = files(args).count();
    let width = column_width(args, &opts, nfiles);
    let mut status = 0;
    let mut total = Counts::default();
    let mut out = Output::new();
    for name in files(args) {
        match cli::with_input(name, |fd| Counts::scan(fd, utf8, gnu_space)) {
            Ok(counts) => {
                total.add(counts);
                if opts.total != Total::Only
                    && let Err(e) = opts.display(&mut out, counts, width, name.map(CStr::to_bytes))
                {
                    cli::diagnostic(b"wc", b"write error", e);
                    return 1;
                }
            }
            Err(error) => {
                cli::diagnostic(b"wc", name.map_or(&b"-"[..], CStr::to_bytes), error);
                status = 1;
            }
        }
    }
    if opts.total == Total::Always
        || opts.total == Total::Auto && nfiles > 1
        || opts.total == Total::Only
    {
        if let Err(e) = opts.display(
            &mut out,
            total,
            width,
            (opts.total != Total::Only).then_some(b"total"),
        ) {
            cli::diagnostic(b"wc", b"write error", e);
            return 1;
        }
    }
    status
}
dosrs_user::utility!(
    run,
    "wc",
    "wc [-cLlmw] [--total=auto|always|only|never] [FILE...]\nCount lines, words, characters, bytes, or maximum display width.",
    0
);
