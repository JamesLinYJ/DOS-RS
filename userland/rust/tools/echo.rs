#![no_std]
use dosrs_user::{
    Args,
    cli::{self, Output},
};

fn emit(out: &mut Output, text: &[u8], escapes: bool) -> Result<bool, i32> {
    if !escapes {
        out.push(text)?;
        return Ok(true);
    }
    let mut i = 0;
    while i < text.len() {
        let byte = text[i];
        i += 1;
        if byte != b'\\' || i == text.len() {
            out.push(&[byte])?;
            continue;
        }
        let escape = text[i];
        i += 1;
        let mapped = match escape {
            b'a' => 7,
            b'b' => 8,
            b'c' => return Ok(false),
            b'e' => 27,
            b'f' => 12,
            b'n' => 10,
            b'r' => 13,
            b't' => 9,
            b'v' => 11,
            b'\\' => b'\\',
            b'0' | b'x' => {
                let radix = if escape == b'0' { 8 } else { 16 };
                let digits = if radix == 8 { 3 } else { 2 };
                let start = i;
                let mut number = 0u32;
                while i < text.len() && i - start < digits {
                    let Some(d) = char::from(text[i]).to_digit(radix) else {
                        break;
                    };
                    number = number * radix + d;
                    i += 1;
                }
                if i == start && escape == b'x' {
                    out.push(b"\\")?;
                    b'x'
                } else {
                    number as u8
                }
            }
            _ => {
                out.push(b"\\")?;
                escape
            }
        };
        out.push(&[mapped])?;
    }
    Ok(true)
}
fn run(args: Args<'_>) -> i32 {
    let mut newline = true;
    let mut escapes = false;
    let mut start = 1;
    while let Some(name) = args.get(start) {
        let bytes = name.to_bytes();
        if bytes.len() < 2 || bytes[0] != b'-' || !bytes[1..].iter().all(|b| b"neE".contains(b)) {
            break;
        }
        for &b in &bytes[1..] {
            match b {
                b'n' => newline = false,
                b'e' => escapes = true,
                _ => escapes = false,
            }
        }
        start += 1;
    }
    let mut out = Output::new();
    let result = (|| {
        for i in start..args.len() {
            if i != start {
                out.push(b" ")?;
            }
            if !emit(&mut out, args.get(i).unwrap().to_bytes(), escapes)? {
                return out.flush();
            }
        }
        if newline {
            out.push(b"\n")?;
        }
        out.flush()
    })();
    if let Err(e) = result {
        cli::diagnostic(b"echo", b"write error", e);
        return 1;
    }
    0
}
dosrs_user::utility!(
    run,
    "echo",
    "echo [-neE] [ARG...]\nWrite the arguments separated by spaces.",
    0
);
