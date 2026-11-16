#![no_std]
#![no_main]
use dos_tools::{drive, input, key, put, submit, text};
fn prompt(buf: &[u8], n: usize) {
    put(65 + drive());
    text(":\\> ");
    let mut i = 0;
    while i < n {
        put(buf[i] as u16);
        i += 1;
    }
}
#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    let mut buf = [0u8; 127];
    let mut n = 0;
    let mut ends = [0u8; 127];
    let mut chars = 0;
    prompt(&buf, n);
    loop {
        let c = key();
        match c {
            13 => {
                text("\r\n");
                submit(&buf, n as u16);
                return 0;
            }
            8 if chars > 0 => {
                chars -= 1;
                let prev = ends[chars] as usize;
                if n - prev > 1 {
                    text("\r\n");
                    prompt(&buf, prev)
                } else {
                    text("\x08 \x08")
                }
                n = prev
            }
            16 | 21 => {
                let mut bytes = [0u8; 4];
                let size = input::compose(c, &mut bytes);
                if size != 0 && n + size <= 126 {
                    ends[chars] = n as u8;
                    chars += 1;
                    let mut i = 0;
                    while i < size {
                        buf[n] = bytes[i];
                        n += 1;
                        i += 1;
                    }
                }
                text("\r\n");
                prompt(&buf, n);
            }
            32..=126 if n < 126 => {
                ends[chars] = n as u8;
                chars += 1;
                buf[n] = c as u8;
                n += 1;
                put(c);
            }
            _ => (),
        }
    }
}
