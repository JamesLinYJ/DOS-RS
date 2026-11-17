#![no_std]
use dosrs_user::{
    Args,
    cli::{self, Output},
};
fn base<'a>(text: &'a [u8], suffix: &[u8]) -> &'a [u8] {
    let Some(end) = text.iter().rposition(|&b| b != b'/') else {
        return if text.is_empty() { b"" } else { b"/" };
    };
    let basename = text[..=end].rsplit(|&b| b == b'/').next().unwrap();
    if suffix.len() < basename.len() && basename.ends_with(suffix) {
        &basename[..basename.len() - suffix.len()]
    } else {
        basename
    }
}

fn run(args: Args<'_>) -> i32 {
    let mut operands = alloc::vec::Vec::new();
    let mut suffix: &[u8] = b"";
    let mut multiple = false;
    let mut end: &[u8] = b"\n";
    let mut options = true;
    let mut i = 1;
    while let Some(arg) = args.get(i) {
        i += 1;
        let s = arg.to_bytes();
        if options && s == b"--" {
            options = false;
            continue;
        }
        if !options || s.len() < 2 || s[0] != b'-' {
            operands.push(s);
            continue;
        }
        match s {
            b"--multiple" => multiple = true,
            b"--zero" => end = b"\0",
            b"--suffix" => {
                let Some(next) = args.get(i) else {
                    return cli::usage_error(
                        b"basename",
                        b"option '--suffix' requires an argument",
                    );
                };
                i += 1;
                suffix = next.to_bytes();
                multiple = true;
            }
            _ if s.starts_with(b"--suffix=") => {
                suffix = &s[9..];
                multiple = true;
            }
            _ if s.starts_with(b"--") => return cli::bad_option(b"basename", &s[2..], true),
            _ => {
                for (index, &flag) in s[1..].iter().enumerate() {
                    match flag {
                        b'a' => multiple = true,
                        b'z' => end = b"\0",
                        b's' => {
                            multiple = true;
                            suffix = if index + 2 < s.len() {
                                &s[index + 2..]
                            } else {
                                let Some(next) = args.get(i) else {
                                    return cli::usage_error(
                                        b"basename",
                                        b"option requires an argument -- 's'",
                                    );
                                };
                                i += 1;
                                next.to_bytes()
                            };
                            break;
                        }
                        _ => {
                            return cli::bad_option(
                                b"basename",
                                core::slice::from_ref(&flag),
                                false,
                            );
                        }
                    }
                }
            }
        }
    }
    if operands.is_empty() {
        return cli::usage_error(b"basename", b"missing operand");
    }
    if !multiple && operands.len() > 2 {
        let mut message = alloc::vec::Vec::from(&b"extra operand '"[..]);
        message.extend_from_slice(operands[2]);
        message.push(b'\'');
        return cli::usage_error(b"basename", &message);
    }
    if !multiple && operands.len() == 2 {
        suffix = operands.pop().unwrap();
    }
    let mut out = Output::new();
    for operand in operands {
        if let Err(e) = out.push(base(operand, suffix)).and_then(|()| out.push(end)) {
            cli::diagnostic(b"basename", b"write error", e);
            return 1;
        }
    }
    if let Err(e) = out.flush() {
        cli::diagnostic(b"basename", b"write error", e);
        return 1;
    }
    0
}
extern crate alloc;
dosrs_user::utility!(
    run,
    "basename",
    "basename [-az] [-s SUFFIX] NAME... | basename NAME [SUFFIX]\nWrite the final component of each pathname.",
    0
);
