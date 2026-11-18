#![no_std]
use dosrs_user::{
    Args,
    cli::{self, Output},
    send_signal,
};

// Signal numbers in the native i686 and x86_64 musl ABI. Musl reserves 32–34.
const STANDARD: [&[u8]; 31] = [
    b"HUP", b"INT", b"QUIT", b"ILL", b"TRAP", b"ABRT", b"BUS", b"FPE", b"KILL", b"USR1", b"SEGV",
    b"USR2", b"PIPE", b"ALRM", b"TERM", b"STKFLT", b"CHLD", b"CONT", b"STOP", b"TSTP", b"TTIN",
    b"TTOU", b"URG", b"XCPU", b"XFSZ", b"VTALRM", b"PROF", b"WINCH", b"POLL", b"PWR", b"SYS",
];
const RT_MIN: i32 = 35;
const RT_MAX: i32 = 64;

fn decimal(raw: &[u8]) -> Option<i32> {
    core::str::from_utf8(raw).ok()?.parse().ok()
}

fn signal(raw: &[u8]) -> Option<i32> {
    if let Some(value) = decimal(raw) {
        return matches!(value, 0..=31 | RT_MIN..=RT_MAX).then_some(value);
    }
    if let Some(index) = STANDARD
        .iter()
        .position(|name| name.eq_ignore_ascii_case(raw))
    {
        return Some(index as i32 + 1);
    }
    for (name, value) in [
        (b"IOT".as_slice(), 6),
        (b"IO", 29),
        (b"RTMIN", RT_MIN),
        (b"RTMAX", RT_MAX),
    ] {
        if name.eq_ignore_ascii_case(raw) {
            return Some(value);
        }
    }
    let (prefix, offset) = (raw.get(..6)?, decimal(raw.get(6..)?)?);
    if !(0..=RT_MAX - RT_MIN).contains(&offset) {
        return None;
    }
    if prefix.eq_ignore_ascii_case(b"RTMIN+") {
        Some(RT_MIN + offset)
    } else if prefix.eq_ignore_ascii_case(b"RTMAX-") {
        Some(RT_MAX - offset)
    } else {
        None
    }
}

fn name(out: &mut Output, number: i32) -> Result<(), i32> {
    match number {
        1..=31 => out.push(STANDARD[number as usize - 1]),
        RT_MIN => out.push(b"RTMIN"),
        RT_MAX => out.push(b"RTMAX"),
        RT_MIN..=RT_MAX => {
            out.push(b"RTMIN+")?;
            out.number((number - RT_MIN) as u64, 0)
        }
        _ => Err(22),
    }
}

fn list<'a>(mut args: impl Iterator<Item = &'a [u8]>) -> i32 {
    let mut out = Output::new();
    let result = (|| {
        if let Some(raw) = args.next() {
            if args.next().is_some() {
                return Err(22);
            }
            let value = decimal(raw).ok_or(22)?;
            name(&mut out, if value > 128 { value - 128 } else { value })?;
        } else {
            for value in (1..=31).chain(RT_MIN..=RT_MAX) {
                if value != 1 {
                    out.push(b" ")?;
                }
                name(&mut out, value)?;
            }
        }
        out.push(b"\n")?;
        out.flush()
    })();
    if let Err(error) = result {
        if error == 22 {
            return cli::usage_error(b"kill", b"expected one signal number or exit status");
        }
        cli::diagnostic(b"kill", b"write error", error);
        return 1;
    }
    0
}

fn run(args: Args<'_>) -> i32 {
    let mut pids = args.operands().map(|arg| arg.to_bytes()).peekable();
    let selected = match pids.peek().copied() {
        Some(b"-l") => {
            pids.next();
            if pids.peek().copied() == Some(b"--") {
                pids.next();
            }
            return list(pids);
        }
        Some(b"-s") => {
            pids.next();
            pids.next().and_then(signal)
        }
        Some(arg) if arg.starts_with(b"-") && arg != b"--" => {
            pids.next();
            signal(&arg[1..]).or_else(|| arg.strip_prefix(b"-s").and_then(signal))
        }
        _ => Some(15),
    };
    let Some(selected) = selected else {
        return cli::usage_error(b"kill", b"invalid signal");
    };
    if pids.peek().copied() == Some(b"--") {
        pids.next();
    }
    if pids.peek().is_none() {
        return cli::usage_error(b"kill", b"expected at least one process ID");
    }
    let mut status = 0;
    for arg in pids {
        let Some(pid) = decimal(arg) else {
            cli::path_diagnostic(b"kill", arg, b"invalid process ID");
            status = 1;
            continue;
        };
        if let Err(error) = send_signal(pid, selected) {
            cli::diagnostic(b"kill", arg, error);
            status = 1;
        }
    }
    status
}

dosrs_user::utility!(
    run,
    "kill",
    "kill [-s SIGNAL | -SIGNAL] [--] PID...\n       kill -l [STATUS]",
    0
);
