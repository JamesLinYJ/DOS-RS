//! Resident DOS command interpreter. All external programs run through ordinary EXEC.
#![no_std]
#![no_main]
#[path = "../command/batch.rs"]
mod batch;
#[path = "../command/builtins.rs"]
mod builtins;
#[path = "../command/dos.rs"]
mod dos;
#[path = "../command/environment.rs"]
mod environment;
#[path = "../command/execute.rs"]
mod execute;
#[path = "../command/loadhigh.rs"]
mod loadhigh;
#[path = "../command/parse.rs"]
mod parse;
#[path = "../command/prompt.rs"]
mod prompt;
struct Shell {
    env: environment::Environment,
    status: u16,
    depth: u16,
    sequence: u16,
    permanent: bool,
    exit: bool,
    echo: bool,
}
#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    let psp = dos::psp();
    if !dos::shrink(psp) {
        dos::err(b"Insufficient command memory\r\n");
        return 8;
    }
    let mut tail = [0u8; parse::LIMIT];
    let len = dos::tail(psp, &mut tail);
    let mut args = parse::left(parse::head(&tail, len));
    let mut permanent = false;
    let mut single = false;
    let mut keep = false;
    while !args.is_empty() {
        if args[0] == b'/' && args.len() >= 2 {
            match parse::upper(args[1]) {
                b'C' | b'K' => {
                    single = true;
                    keep = parse::upper(args[1]) == b'K';
                    args = parse::left(parse::rest(&args, 2));
                    break;
                }
                b'P' => permanent = true,
                _ => (),
            }
        }
        args = parse::word(args).1;
    }
    let Some(env) = environment::Environment::new(psp, 2048) else {
        dos::err(b"Out of environment space\r\n");
        return 8;
    };
    let mut shell = Shell {
        env,
        status: 0,
        depth: 0,
        sequence: 0,
        permanent,
        exit: false,
        echo: true,
    };
    if permanent {
        builtins::run(b"VER", b"", false);
        if dos::exists(b"\\AUTOEXEC.BAT") {
            batch::run(&mut shell, b"\\AUTOEXEC.BAT", b"");
        }
    }
    if single {
        let mut frame = batch::Frame::new(0xffff, b"", b"");
        let mut expanded = [0; parse::LIMIT];
        if let Some(n) = batch::expand(&shell, &frame, args, &mut expanded) {
            execute::line(&mut shell, &mut frame, parse::head(&expanded, n), false);
        } else {
            execute::syntax(&mut shell);
        }
        if !keep {
            return shell.status;
        }
    }
    execute::interactive(&mut shell);
    shell.status
}
