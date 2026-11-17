use super::{dos, environment::Environment, parse};
fn pair(n: u16) {
    dos::out(&[b'0' + (n / 10 % 10) as u8, b'0' + (n % 10) as u8]);
}
pub fn display(env: &Environment) {
    let mut buf = [0; 128];
    let format: &[u8] = if let Some(n) = env.get(b"PROMPT", &mut buf) {
        parse::head(&buf, n)
    } else {
        b"$P$G "
    };
    let mut i = 0;
    while i < format.len() {
        if format[i] != b'$' {
            let from = i;
            while i < format.len() && format[i] != b'$' {
                i += 1;
            }
            dos::out(parse::part(&format, from, i));
            continue;
        }
        i += 1;
        if i == format.len() {
            break;
        }
        let c = parse::upper(format[i]);
        i += 1;
        match c {
            b'P' | b'N' => {
                let drive = dos::drive();
                dos::out(&[b'A' + drive as u8]);
                if c == b'P' {
                    dos::out(b":\\");
                    let mut dir = [0u8; 68];
                    let n = dos::cwd(drive, &mut dir);
                    dos::out(parse::head(&dir, n));
                }
            }
            b'G' => dos::out(b">"),
            b'L' => dos::out(b"<"),
            b'B' => dos::out(b"|"),
            b'Q' => dos::out(b"="),
            b'H' => dos::out(b"\x08"),
            b'E' => dos::out(b"\x1b"),
            b'$' => dos::out(b"$"),
            b'_' => dos::out(b"\r\n"),
            b'V' => {
                let v = dos::service(0x3000, 0, 0, 0).unwrap_or(0);
                dos::out(b"DOS ");
                dos::out(&[b'0' + (v % 256 % 10) as u8]);
                dos::out(b".");
                pair(v >> 8);
            }
            b'D' | b'T' => {
                let mut r = [0; 12];
                r[0] = if c == b'D' { 0x2a00 } else { 0x2c00 };
                dos::call(&mut r);
                if c == b'D' {
                    pair(r[3] >> 8);
                    dos::out(b"-");
                    pair(r[3] & 255);
                    dos::out(b"-");
                    pair(r[2] / 100);
                    pair(r[2] % 100);
                } else {
                    pair(r[2] >> 8);
                    dos::out(b":");
                    pair(r[2] & 255);
                    dos::out(b":");
                    pair(r[3] >> 8);
                    dos::out(b".");
                    pair(r[3] & 255);
                }
            }
            _ => (),
        }
    }
}
