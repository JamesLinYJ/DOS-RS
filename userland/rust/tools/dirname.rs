#![no_std]
use dosrs_user::{
    Args,
    cli::{self, Arg, Output},
};
fn directory(text: &[u8]) -> &[u8] {
    let trimmed = trim_slashes(text);
    match trimmed.iter().rposition(|&b| b == b'/') {
        Some(index) => {
            let dir = trim_slashes(&trimmed[..index]);
            if dir.is_empty() { b"/" } else { dir }
        }
        None if !text.is_empty() && trimmed.is_empty() => b"/",
        None => b".",
    }
}
fn trim_slashes(path: &[u8]) -> &[u8] {
    path.iter()
        .rposition(|&b| b != b'/')
        .map_or(&[], |i| &path[..=i])
}
fn run(args: Args<'_>) -> i32 {
    let mut end: &[u8] = b"\n";
    for arg in cli::arguments(args) {
        match arg {
            Arg::Short(flags) => {
                for flag in flags {
                    if *flag != b'z' {
                        return cli::bad_option(b"dirname", core::slice::from_ref(flag), false);
                    }
                    end = b"\0";
                }
            }
            Arg::Long(b"zero") => end = b"\0",
            Arg::Long(name) => return cli::bad_option(b"dirname", name, true),
            Arg::Operand(_) => (),
        }
    }
    if cli::operands(args).next().is_none() {
        return cli::usage_error(b"dirname", b"missing operand");
    }
    let mut out = Output::new();
    for name in cli::operands(args) {
        if let Err(e) = out
            .push(directory(name.to_bytes()))
            .and_then(|()| out.push(end))
        {
            cli::diagnostic(b"dirname", b"write error", e);
            return 1;
        }
    }
    if let Err(e) = out.flush() {
        cli::diagnostic(b"dirname", b"write error", e);
        return 1;
    }
    0
}
dosrs_user::utility!(
    run,
    "dirname",
    "dirname [-z] NAME...\nWrite the directory part of each pathname.",
    0
);
