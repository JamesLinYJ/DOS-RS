//! Common byte-oriented argument, diagnostic, and buffered output helpers.
use crate::{Args, close, open, write_all};
use core::ffi::CStr;

pub enum Arg<'a> {
    Short(&'a [u8]),
    Long(&'a [u8]),
    Operand(&'a CStr),
}

pub fn arguments(args: Args<'_>) -> impl Iterator<Item = Arg<'_>> {
    let mut options = true;
    (1..args.len()).filter_map(move |index| {
        let name = args.get(index)?;
        let bytes = name.to_bytes();
        if options && bytes == b"--" {
            options = false;
            return None;
        }
        if options && bytes.starts_with(b"--") {
            Some(Arg::Long(&bytes[2..]))
        } else if options && bytes.len() > 1 && bytes[0] == b'-' {
            Some(Arg::Short(&bytes[1..]))
        } else {
            Some(Arg::Operand(name))
        }
    })
}

pub fn operands(args: Args<'_>) -> impl Iterator<Item = &CStr> {
    arguments(args).filter_map(|a| match a {
        Arg::Operand(n) => Some(n),
        _ => None,
    })
}

pub fn with_input<T>(
    path: Option<&CStr>,
    read_input: impl FnOnce(i32) -> Result<T, i32>,
) -> Result<T, i32> {
    match path {
        Some(p) if p.to_bytes() != b"-" => {
            let fd = open(p, 0)?;
            let result = read_input(fd);
            let closed = close(fd);
            result.and_then(|value| closed.map(|()| value))
        }
        _ => read_input(0),
    }
}

pub fn diagnostic(tool: &[u8], path: &[u8], errno: i32) {
    let message: &[u8] = match errno {
        1 => b"Operation not permitted",
        2 => b"No such file or directory",
        3 => b"No such process",
        5 => b"Input/output error",
        9 => b"Bad file descriptor",
        12 => b"Cannot allocate memory",
        13 => b"Permission denied",
        17 => b"File exists",
        18 => b"Invalid cross-device link",
        20 => b"Not a directory",
        21 => b"Is a directory",
        22 => b"Invalid argument",
        28 => b"No space left on device",
        30 => b"Read-only file system",
        32 => b"Broken pipe",
        34 => b"Result too large",
        36 => b"File name too long",
        39 => b"Directory not empty",
        40 => b"Too many levels of symbolic links",
        84 => b"Invalid or incomplete multibyte or wide character",
        95 => b"Operation not supported",
        _ => b"Unknown error",
    };
    path_diagnostic(tool, path, message);
}

pub fn path_diagnostic(tool: &[u8], path: &[u8], message: &[u8]) {
    for part in [tool, b": ", path, b": ", message, b"\n"] {
        let _ = write_all(2, part);
    }
}

pub fn bad_option(tool: &[u8], value: &[u8], long: bool) -> i32 {
    let pre: &[u8] = if long {
        b": unrecognized option '--"
    } else {
        b": invalid option -- '"
    };
    for b in [
        tool,
        pre,
        value,
        b"'\nTry '",
        tool,
        b" --help' for more information.\n",
    ] {
        let _ = write_all(2, b);
    }
    1
}

/// Bounded buffering keeps large transformations from emitting one syscall per byte.
pub struct Output {
    buf: [u8; 2048],
    used: usize,
}
impl Output {
    pub fn new() -> Self {
        Self {
            buf: [0; 2048],
            used: 0,
        }
    }
    pub fn push(&mut self, mut bytes: &[u8]) -> Result<(), i32> {
        while !bytes.is_empty() {
            let n = bytes.len().min(self.buf.len() - self.used);
            self.buf[self.used..self.used + n].copy_from_slice(&bytes[..n]);
            self.used += n;
            bytes = &bytes[n..];
            if self.used == self.buf.len() {
                self.flush()?;
            }
        }
        Ok(())
    }
    pub fn flush(&mut self) -> Result<(), i32> {
        write_all(1, &self.buf[..self.used])?;
        self.used = 0;
        Ok(())
    }
    pub fn number(&mut self, mut value: u64, width: usize) -> Result<(), i32> {
        let mut buf = [b' '; 20];
        let mut i = buf.len();
        loop {
            i -= 1;
            buf[i] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        self.push(&buf[i.min(buf.len() - width.min(buf.len()))..])
    }
}

pub fn usage_error(tool: &[u8], message: &[u8]) -> i32 {
    for part in [
        tool,
        b": ",
        message,
        b"\nTry '",
        tool,
        b" --help' for more information.\n",
    ] {
        let _ = write_all(2, part);
    }
    1
}

pub fn information(args: Args<'_>, tool: &str, help: &str, status: i32) -> Option<i32> {
    if args.len() != 2 {
        return None;
    }
    let parts: &[&[u8]] = match args.get(1)?.to_bytes() {
        b"--help" => &[b"Usage: ", help.as_bytes(), b"\n"],
        b"--version" => &[
            tool.as_bytes(),
            b" (DOS-RS) ",
            env!("CARGO_PKG_VERSION").as_bytes(),
            b"\n",
        ],
        _ => return None,
    };
    for part in parts {
        if write_all(1, part).is_err() {
            return Some(1);
        }
    }
    Some(status)
}
