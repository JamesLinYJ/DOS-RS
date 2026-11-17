#![no_std]

use dosrs_user::{
    Args,
    cli::{self, Arg, Output},
    read, write_all,
};

#[derive(Default)]
struct Options {
    number: bool,
    nonblank: bool,
    squeeze: bool,
    ends: bool,
    tabs: bool,
    visible: bool,
}
impl Options {
    fn set(&mut self, flag: u8) -> bool {
        match flag {
            b'b' => self.nonblank = true,
            b'n' => self.number = true,
            b's' => self.squeeze = true,
            b'E' => self.ends = true,
            b'T' => self.tabs = true,
            b'v' => self.visible = true,
            b'A' => {
                self.ends = true;
                self.tabs = true;
                self.visible = true;
            }
            b'e' => {
                self.ends = true;
                self.visible = true;
            }
            b't' => {
                self.tabs = true;
                self.visible = true;
            }
            b'u' => (),
            _ => return false,
        }
        true
    }
    fn plain(&self) -> bool {
        !(self.number || self.nonblank || self.squeeze || self.ends || self.tabs || self.visible)
    }
}

struct State {
    at_start: bool,
    previous_blank: bool,
    cr: bool,
    line: u64,
}
impl State {
    fn byte(&mut self, byte: u8, opts: &Options, out: &mut Output) -> Result<(), i32> {
        if self.cr {
            out.push(if byte == b'\n' { b"^M" } else { b"\r" })?;
            self.cr = false;
        }
        let blank = self.at_start && byte == b'\n';
        if opts.squeeze && blank && self.previous_blank {
            return Ok(());
        }
        if self.at_start && (opts.nonblank && !blank || !opts.nonblank && opts.number) {
            self.line += 1;
            out.number(self.line, 6)?;
            out.push(b"\t")?;
        }
        self.at_start = byte == b'\n';
        self.previous_blank = blank;
        match byte {
            b'\n' if opts.ends => out.push(b"$\n"),
            b'\n' => out.push(b"\n"),
            b'\t' if opts.tabs => out.push(b"^I"),
            b'\t' => out.push(b"\t"),
            b'\r' if opts.ends && !opts.visible => {
                self.cr = true;
                Ok(())
            }
            _ if opts.visible => {
                if byte >= 128 {
                    out.push(b"M-")?;
                }
                let ch = byte & 127;
                match ch {
                    0..32 => out.push(&[b'^', ch + 64]),
                    127 => out.push(b"^?"),
                    _ => out.push(&[ch]),
                }
            }
            _ => out.push(&[byte]),
        }
    }
}

fn run(args: Args<'_>) -> i32 {
    let mut opts = Options::default();
    for arg in cli::arguments(args) {
        match arg {
            Arg::Short(flags) => {
                for flag in flags {
                    if !opts.set(*flag) {
                        return cli::bad_option(b"cat", core::slice::from_ref(flag), false);
                    }
                }
            }
            Arg::Long(name) => {
                let flag = match name {
                    b"show-all" => b'A',
                    b"number-nonblank" => b'b',
                    b"show-ends" => b'E',
                    b"number" => b'n',
                    b"squeeze-blank" => b's',
                    b"show-tabs" => b'T',
                    b"show-nonprinting" => b'v',
                    _ => return cli::bad_option(b"cat", name, true),
                };
                opts.set(flag);
            }
            Arg::Operand(_) => (),
        }
    }
    let mut out = Output::new();
    let mut state = State {
        at_start: true,
        previous_blank: false,
        cr: false,
        line: 0,
    };
    let dest = dosrs_user::stat::Stat::fd(1)
        .ok()
        .filter(|st| st.mode & 0o170000 == 0o100000);
    let mut status = 0;
    let mut data = [0; 4096];
    let mut files = cli::operands(args).map(Some).peekable();
    let stdin = files.peek().is_none().then_some(None);
    for file in stdin.into_iter().chain(files) {
        let mut writing = false;
        let result = cli::with_input(file, |fd| {
            if let Some(dst) = &dest
                && let Ok(src) = dosrs_user::stat::Stat::fd(fd)
                && src.dev == dst.dev
                && src.ino == dst.ino
                && src.size > 0
            {
                let label = file.map_or(&b"-"[..], |p| p.to_bytes());
                cli::path_diagnostic(b"cat", label, b"input file is output file");
                status = 1;
                return Ok(());
            }
            loop {
                let count = read(fd, &mut data)?;
                if count == 0 {
                    return Ok(());
                }
                writing = true;
                if opts.plain() {
                    write_all(1, &data[..count])?;
                } else {
                    for &b in &data[..count] {
                        state.byte(b, &opts, &mut out)?;
                    }
                    out.flush()?;
                }
                writing = false;
            }
        });
        if let Err(error) = result {
            let label: &[u8] = if writing {
                b"write error"
            } else {
                file.map_or(&b"-"[..], |p| p.to_bytes())
            };
            cli::diagnostic(b"cat", label, error);
            if writing {
                return 1;
            }
            status = 1;
        }
    }
    if state.cr
        && let Err(e) = write_all(1, b"\r")
    {
        cli::diagnostic(b"cat", b"write error", e);
        return 1;
    }
    status
}
dosrs_user::utility!(
    run,
    "cat",
    "cat [-AbEenstTuv] [FILE...]\nCopy each FILE, or standard input, to standard output.",
    0
);
