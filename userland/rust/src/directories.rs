//! Directory utilities use the POSIX namespace and process umask supplied by libc.
extern crate alloc;
use crate::{Args, cli, errno, stat::Stat, write_all};
use alloc::{ffi::CString, vec::Vec};
use core::ffi::CStr;

unsafe extern "C" {
    fn mkdir(path: *const i8, mode: u32) -> i32;
    fn chmod(path: *const i8, mode: u32) -> i32;
    fn rmdir(path: *const i8) -> i32;
    fn umask(mask: u32) -> u32;
}

fn mode_value(s: &[u8], mask: u32) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    if s[0].is_ascii_digit() {
        let mut n = 0u32;
        for &b in s {
            if !(b'0'..=b'7').contains(&b) {
                return None;
            }
            n = n.checked_mul(8)?.checked_add((b - b'0') as u32)?;
        }
        return (n <= 0o7777).then_some(n);
    }
    let mut mode = 0o777u32;
    for clause in s.split(|&b| b == b',') {
        let mut idx = 0;
        let mut who = 0;
        for &b in clause {
            match b {
                b'u' => who |= 0o4700,
                b'g' => who |= 0o2070,
                b'o' => who |= 0o1007,
                b'a' => who = 0o7777,
                _ => break,
            };
            idx += 1;
        }
        let explicit = who != 0;
        if !explicit {
            who = 0o7777;
        }
        if idx == clause.len() {
            return None;
        }
        while idx < clause.len() {
            let op = clause[idx];
            if !b"+-=".contains(&op) {
                return None;
            }
            idx += 1;
            let start = idx;
            while idx < clause.len() && !b"+-=".contains(&clause[idx]) {
                idx += 1;
            }
            let bits = &clause[start..idx];
            let mut perm = 0;
            if bits.len() == 1 && b"ugo".contains(&bits[0]) {
                let shift = match bits[0] {
                    b'u' => 6,
                    b'g' => 3,
                    _ => 0,
                };
                let r = (mode >> shift) & 7;
                perm = r * 0o111;
            } else {
                for &b in bits {
                    perm |= match b {
                        b'r' => 0o444,
                        b'w' => 0o222,
                        b'x' | b'X' => 0o111,
                        b's' => 0o6000,
                        b't' => 0o1000,
                        _ => return None,
                    };
                }
            }
            perm &= who;
            if !explicit {
                perm &= !mask;
            }
            match op {
                b'+' => mode |= perm,
                b'-' => mode &= !perm,
                _ => mode = (mode & !who) | perm,
            }
        }
    }
    Some(mode)
}

struct Options<'a> {
    parents: bool,
    verbose: bool,
    ignore_nonempty: bool,
    mode: Option<u32>,
    names: Vec<&'a CStr>,
}
fn parse<'a>(args: Args<'a>, cmd: &[u8], remove: bool, mask: u32) -> Result<Options<'a>, i32> {
    let mut o = Options {
        parents: false,
        verbose: false,
        ignore_nonempty: false,
        mode: None,
        names: Vec::new(),
    };
    let mut at = 1;
    let mut flags = true;
    while let Some(p) = args.get(at) {
        at += 1;
        let arg = p.to_bytes();
        if flags && arg == b"--" {
            flags = false;
            continue;
        }
        if !flags || arg.len() < 2 || arg[0] != b'-' {
            o.names.push(p);
            continue;
        }
        let mut mode = None;
        if let Some(key) = arg.strip_prefix(b"--") {
            match key {
                b"parents" => o.parents = true,
                b"verbose" => o.verbose = true,
                b"ignore-fail-on-non-empty" if remove => o.ignore_nonempty = true,
                b"mode" if !remove => {
                    mode = Some(
                        args.get(at)
                            .ok_or_else(|| {
                                cli::usage_error(cmd, b"option '--mode' requires an argument")
                            })?
                            .to_bytes(),
                    );
                    at += 1;
                }
                _ if !remove && key.starts_with(b"mode=") => mode = Some(&key[5..]),
                _ => return Err(cli::bad_option(cmd, key, true)),
            }
        } else {
            for (i, &b) in arg.iter().enumerate().skip(1) {
                match b {
                    b'p' => o.parents = true,
                    b'v' => o.verbose = true,
                    b'm' if !remove => {
                        mode = Some(if i + 1 < arg.len() {
                            &arg[i + 1..]
                        } else {
                            let next = args.get(at).ok_or_else(|| {
                                cli::usage_error(cmd, b"option requires an argument -- 'm'")
                            })?;
                            at += 1;
                            next.to_bytes()
                        });
                        break;
                    }
                    _ => return Err(cli::bad_option(cmd, core::slice::from_ref(&b), false)),
                }
            }
        }
        if let Some(value) = mode {
            o.mode = Some(mode_value(value, mask).ok_or_else(|| {
                for part in [cmd, b": invalid mode '", value, b"'\n"] {
                    let _ = write_all(2, part);
                }
                1
            })?);
        }
    }
    if o.names.is_empty() {
        return Err(cli::usage_error(cmd, b"missing operand"));
    }
    Ok(o)
}
fn parts(path: &CStr, parents: bool) -> Vec<&[u8]> {
    let s = path.to_bytes();
    let mut list = Vec::new();
    if parents && let Some(end) = s.iter().rposition(|&b| b != b'/') {
        for (i, &b) in s[..end].iter().enumerate() {
            if b == b'/' && i > 0 && s[i - 1] != b'/' {
                list.push(&s[..i]);
            }
        }
    }
    list.push(s);
    list
}
fn message(cmd: &[u8], pre: &[u8], path: &[u8], err: Option<i32>) {
    let mut label = Vec::from(pre);
    label.push(b'\'');
    label.extend_from_slice(path);
    label.push(b'\'');
    if let Some(e) = err {
        cli::diagnostic(cmd, &label, e);
    } else {
        for part in [cmd, b": ", &label, b"\n"] {
            let _ = write_all(1, part);
        }
    }
}
fn create(name: &CStr, o: &Options<'_>, mask: u32) -> Result<(), ()> {
    let parts = parts(name, o.parents);
    for (index, part) in parts.iter().enumerate() {
        let path = CString::new(*part).unwrap();
        let final_part = index + 1 == parts.len();
        let mode = if final_part {
            o.mode.unwrap_or(0o777 & !mask)
        } else {
            (0o777 & !mask) | 0o300
        };
        if unsafe { mkdir(path.as_ptr(), mode) } != 0 {
            let error = errno();
            if o.parents
                && error == 17
                && Stat::file(Some(&path)).is_ok_and(|s| s.mode & 0o170000 == 0o040000)
            {
                continue;
            }
            message(b"mkdir", b"cannot create directory ", part, Some(error));
            return Err(());
        }
        if o.verbose {
            message(b"mkdir", b"created directory ", part, None);
        }
        if final_part && o.mode.is_some() && unsafe { chmod(path.as_ptr(), mode) } != 0 {
            message(b"mkdir", b"cannot set permissions of ", part, Some(errno()));
            return Err(());
        }
    }
    Ok(())
}
fn remove(name: &CStr, o: &Options<'_>) -> Result<(), ()> {
    for part in parts(name, o.parents).into_iter().rev() {
        let path = CString::new(part).unwrap();
        if o.verbose {
            message(b"rmdir", b"removing directory, ", part, None);
        }
        if unsafe { rmdir(path.as_ptr()) } != 0 {
            let e = errno();
            if e == 39 && o.ignore_nonempty {
                return Ok(());
            }
            message(b"rmdir", b"failed to remove ", part, Some(e));
            return Err(());
        }
    }
    Ok(())
}

pub fn run(args: Args<'_>, remove_directory: bool) -> i32 {
    let cmd: &[u8] = if remove_directory { b"rmdir" } else { b"mkdir" };
    // The CLI is single-threaded. Calculate permissions explicitly so -p can
    // grant owner write/search on intermediate directories even with a strict mask.
    let mask = if remove_directory {
        0
    } else {
        unsafe { umask(0) }
    };
    let o = match parse(args, cmd, remove_directory, mask) {
        Ok(o) => o,
        Err(code) => return code,
    };
    let mut status = 0;
    for &name in &o.names {
        let result = if remove_directory {
            remove(name, &o)
        } else {
            create(name, &o, mask)
        };
        if result.is_err() {
            status = 1;
        }
    }
    status
}
