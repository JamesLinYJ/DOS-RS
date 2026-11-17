//! Streaming line and byte selection shared by head and tail.
extern crate alloc;
use crate::{
    Args,
    cli::{self, Output},
    read, seek, write_all,
};
use alloc::{collections::VecDeque, vec::Vec};
use core::ffi::CStr;

#[derive(Clone, Copy)]
pub enum End {
    Head,
    Tail,
}

#[derive(Clone, Copy)]
enum Selection {
    First,
    Skip,
    Last,
    ExceptLast,
}
struct Options<'a> {
    count: usize,
    bytes: bool,
    selection: Selection,
    delimiter: u8,
    headers: Option<bool>,
    files: Vec<Option<&'a CStr>>,
}
fn set_count(opts: &mut Options<'_>, tool: End, value: &[u8], bytes: bool) -> Result<(), ()> {
    let (sign, number) = match value.first() {
        Some(b'+') | Some(b'-') => (value[0], &value[1..]),
        _ => (0, value),
    };
    let digits = number.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return Err(());
    }
    let mut n = 0usize;
    for &b in &number[..digits] {
        n = n.saturating_mul(10).saturating_add(usize::from(b - b'0'));
    }
    let suffix = &number[digits..];
    if !suffix.is_empty() {
        let multiplier = match suffix {
            b"b" => 512,
            _ => {
                let unit = b"KMGTPEZYRQ"
                    .iter()
                    .position(|v| *v == suffix[0].to_ascii_uppercase())
                    .ok_or(())?;
                let base: usize = match &suffix[1..] {
                    b"" | b"iB" => 1024,
                    b"B" => 1000,
                    _ => return Err(()),
                };
                base.saturating_pow(unit as u32 + 1)
            }
        };
        n = n.saturating_mul(multiplier);
    }
    opts.count = n;
    opts.bytes = bytes;
    opts.selection = match (tool, sign) {
        (End::Head, b'-') => Selection::ExceptLast,
        (End::Head, _) => Selection::First,
        (End::Tail, b'+') => {
            opts.count = n.saturating_sub(1);
            Selection::Skip
        }
        (End::Tail, _) => Selection::Last,
    };
    Ok(())
}

fn count_option(
    opts: &mut Options<'_>,
    tool: End,
    cmd: &[u8],
    value: &[u8],
    bytes: bool,
) -> Result<(), i32> {
    if set_count(opts, tool, value, bytes).is_ok() {
        return Ok(());
    }
    for part in [
        cmd,
        &b": invalid number of "[..],
        if bytes { b"bytes" } else { b"lines" },
        b": '",
        value,
        b"'\n",
    ] {
        let _ = write_all(2, part);
    }
    Err(1)
}

fn options<'a>(args: Args<'a>, tool: End, cmd: &[u8]) -> Result<Options<'a>, i32> {
    let mut o = Options {
        count: 10,
        bytes: false,
        selection: match tool {
            End::Head => Selection::First,
            End::Tail => Selection::Last,
        },
        delimiter: b'\n',
        headers: None,
        files: Vec::new(),
    };
    let mut at = 1;
    let mut flags = true;
    while let Some(name) = args.get(at) {
        at += 1;
        let arg = name.to_bytes();
        if flags && arg == b"--" {
            flags = false;
            continue;
        }
        if !flags || arg.len() < 2 || arg[0] != b'-' {
            o.files.push(Some(name));
            continue;
        }
        if at == 2 && arg[1].is_ascii_digit() {
            count_option(&mut o, tool, cmd, &arg[1..], false)?;
            continue;
        }
        if let Some(long) = arg.strip_prefix(b"--") {
            let k = long.iter().position(|&b| b == b'=').unwrap_or(long.len());
            let has_value = k < long.len();
            match &long[..k] {
                b"bytes" | b"lines" => {
                    let bytes = &long[..k] == b"bytes";
                    let value = if has_value {
                        &long[k + 1..]
                    } else {
                        let next = args.get(at).ok_or_else(|| {
                            cli::usage_error(
                                cmd,
                                if bytes {
                                    b"option '--bytes' requires an argument"
                                } else {
                                    b"option '--lines' requires an argument"
                                },
                            )
                        })?;
                        at += 1;
                        next.to_bytes()
                    };
                    count_option(&mut o, tool, cmd, value, bytes)?;
                }
                b"silent" | b"quiet" if !has_value => o.headers = Some(false),
                b"verbose" if !has_value => o.headers = Some(true),
                b"zero-terminated" if !has_value => o.delimiter = 0,
                _ => return Err(cli::bad_option(cmd, long, true)),
            }
            continue;
        }
        for (i, &flag) in arg.iter().enumerate().skip(1) {
            match flag {
                b'c' | b'n' => {
                    let bytes = flag == b'c';
                    let value = if i + 1 < arg.len() {
                        &arg[i + 1..]
                    } else {
                        let next = args.get(at).ok_or_else(|| {
                            cli::usage_error(
                                cmd,
                                if bytes {
                                    b"option requires an argument -- 'c'"
                                } else {
                                    b"option requires an argument -- 'n'"
                                },
                            )
                        })?;
                        at += 1;
                        next.to_bytes()
                    };
                    count_option(&mut o, tool, cmd, value, bytes)?;
                    break;
                }
                b'q' => o.headers = Some(false),
                b'v' => o.headers = Some(true),
                b'z' => o.delimiter = 0,
                _ => return Err(cli::bad_option(cmd, core::slice::from_ref(&flag), false)),
            }
        }
    }
    if o.files.is_empty() {
        o.files.push(None);
    }
    Ok(o)
}

fn forward(fd: i32, out: &mut Output, o: &Options<'_>, first: bool) -> Result<(), i32> {
    let mut remaining = o.count;
    let mut buf = [0; 4096];
    loop {
        if first && remaining == 0 {
            return Ok(());
        }
        let max = if o.bytes && first {
            remaining.min(buf.len())
        } else {
            buf.len()
        };
        let n = read(fd, &mut buf[..max])?;
        if n == 0 {
            return Ok(());
        }
        let mut at = 0;
        if o.bytes {
            at = remaining.min(n);
            remaining -= at;
        } else {
            while at < n && remaining > 0 {
                let end = buf[at..n].iter().position(|&b| b == o.delimiter);
                if let Some(k) = end {
                    at += k + 1;
                    remaining -= 1;
                } else {
                    at = n;
                }
            }
        }
        if first {
            out.push(&buf[..at])?;
            // Leave unread bytes for a following reader of seekable standard input.
            if at < n {
                let _ = seek(fd, -((n - at) as i64), 1);
                return Ok(());
            }
        } else {
            out.push(&buf[at..n])?;
        }
    }
}

fn from_end(fd: i32, out: &mut Output, o: &Options<'_>, drop_end: bool) -> Result<(), i32> {
    if o.count == 0 {
        if !drop_end {
            return Ok(());
        }
        let mut copy = [0; 4096];
        loop {
            let n = read(fd, &mut copy)?;
            if n == 0 {
                return Ok(());
            }
            out.push(&copy[..n])?;
        }
    }
    let mut held = VecDeque::<u8>::new();
    let mut records = 0usize;
    let mut starts_record = true;
    let mut data = [0; 4096];
    loop {
        let n = read(fd, &mut data)?;
        if n == 0 {
            break;
        }
        for &b in &data[..n] {
            if o.bytes || starts_record {
                if records == o.count {
                    while let Some(old) = held.pop_front() {
                        if drop_end {
                            out.push(&[old])?;
                        }
                        if o.bytes || old == o.delimiter {
                            break;
                        }
                    }
                    records -= 1;
                }
                records += 1;
            }
            held.try_reserve(1).map_err(|_| 12)?;
            held.push_back(b);
            starts_record = b == o.delimiter;
        }
    }
    if !drop_end {
        let (a, b) = held.as_slices();
        out.push(a)?;
        out.push(b)?;
    }
    Ok(())
}

pub fn run(args: Args<'_>, tool: End) -> i32 {
    let cmd: &[u8] = match tool {
        End::Head => b"head",
        End::Tail => b"tail",
    };
    let o = match options(args, tool, cmd) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let headers = o.headers.unwrap_or(o.files.len() > 1);
    let mut printed = false;
    let mut status = 0;
    let mut out = Output::new();
    for &file in &o.files {
        let result = cli::with_input(file, |fd| {
            if headers {
                if printed {
                    out.push(b"\n")?;
                }
                printed = true;
                let name = file
                    .filter(|p| p.to_bytes() != b"-")
                    .map_or(&b"'standard input'"[..], CStr::to_bytes);
                out.push(b"==> ")?;
                out.push(name)?;
                out.push(b" <==\n")?;
            }
            match o.selection {
                Selection::First => forward(fd, &mut out, &o, true),
                Selection::Skip => forward(fd, &mut out, &o, false),
                Selection::Last => from_end(fd, &mut out, &o, false),
                Selection::ExceptLast => from_end(fd, &mut out, &o, true),
            }?;
            out.flush()
        });
        if let Err(e) = result {
            let name = file.map_or(&b"-"[..], CStr::to_bytes);
            if e == 2 {
                let mut label = Vec::from(&b"cannot open '"[..]);
                label.extend_from_slice(name);
                label.extend_from_slice(b"' for reading");
                cli::diagnostic(cmd, &label, e);
            } else {
                cli::diagnostic(cmd, name, e);
            }
            status = 1;
        }
    }
    status
}
