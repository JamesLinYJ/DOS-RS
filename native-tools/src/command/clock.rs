use super::*;
fn input(out: &mut [u8]) -> usize {
    let mut n = 0;
    let mut b = [0];
    while let Ok(1) = dos::read(0, &mut b) {
        if matches!(b[0], 13 | 10 | 26) {
            break;
        }
        if b[0] == 8 {
            if n > 0 {
                n -= 1;
            }
        } else if n < out.len() {
            out[n] = b[0];
            n += 1;
        }
    }
    newline();
    n
}
fn parse_time(date: bool, s: &[u8], r: &mut [u16; 12]) -> bool {
    let mut values = [0u16; 4];
    let mut field = 0;
    let mut i = 0;
    while field < 4 && i < s.len() {
        let begin = i;
        let mut val = 0u16;
        while i < s.len() && s[i] >= b'0' && s[i] <= b'9' {
            if val > 999 {
                return false;
            }
            val = val * 10 + u16::from(s[i] - b'0');
            i += 1;
        }
        if i == begin {
            return false;
        }
        if !date && field == 3 && i == begin + 1 {
            val *= 10;
        }
        values[field] = val;
        field += 1;
        if i == s.len() {
            break;
        }
        if !date && matches!(upper(s[i]), b'A' | b'P') {
            break;
        }
        if !(if date {
            matches!(s[i], b'/' | b'-' | b'.')
        } else if field < 3 {
            s[i] == b':'
        } else {
            s[i] == b'.'
        }) {
            return false;
        }
        i += 1;
        if i == s.len() {
            return false;
        }
    }
    if date {
        if i != s.len() || field != 3 || values[0] > 12 || values[1] > 31 {
            return false;
        }
        let y = values[2];
        r[0] = 0x2b00;
        r[2] = if y < 80 {
            2000 + y
        } else if y < 100 {
            1900 + y
        } else {
            y
        };
        r[3] = (values[0] << 8) | values[1];
    } else {
        if field < 2 {
            return false;
        }
        let mut h = values[0];
        if i < s.len() {
            let suffix = parse::rest(s, i);
            if !matches!(h, 1..=12) {
                return false;
            }
            if equal(suffix, b"A") || equal(suffix, b"AM") {
                h %= 12;
            } else if equal(suffix, b"P") || equal(suffix, b"PM") {
                h = h % 12 + 12;
            } else {
                return false;
            }
        }
        if h >= 24 || values[1] >= 60 || values[2] >= 60 || values[3] >= 100 {
            return false;
        }
        r[0] = 0x2d00;
        r[2] = (h << 8) | values[1];
        r[3] = (values[2] << 8) | values[3];
    }
    true
}
pub(super) fn run(date: bool, args: &[u8]) -> u16 {
    let mut r = [0; 12];
    r[0] = if date { 0x2a00 } else { 0x2c00 };
    if !dos::call(&mut r) {
        return message(r[0]);
    }
    let mut buf = [0; 32];
    let mut value = args;
    if value.is_empty() {
        if date {
            let day = r[0] & 255;
            dos::out(b"Current date is ");
            if day < 7 {
                let d = usize::from(day) * 3;
                dos::out(parse::part(b"SunMonTueWedThuFriSat", d, d + 3));
                byte(b' ');
            }
            two(r[3] >> 8);
            byte(b'-');
            two(r[3] & 255);
            byte(b'-');
            number(u32::from(r[2]), 0);
            dos::out(b"\r\nEnter new date (mm-dd-yy): ");
        } else {
            let h = r[2] >> 8;
            let hr = h % 12;
            dos::out(b"Current time is ");
            number(u32::from(if hr == 0 { 12 } else { hr }), 2);
            byte(b':');
            two(r[2] & 255);
            byte(b':');
            two(r[3] >> 8);
            byte(b'.');
            two(r[3] & 255);
            byte(if h >= 12 { b'p' } else { b'a' });
            dos::out(b"\r\nEnter new time: ");
        }
        let n = input(&mut buf);
        if n == 0 {
            return 0;
        }
        value = parse::head(&buf, n);
    }
    if parse_time(date, value, &mut r) && dos::call(&mut r) && r[0] & 255 == 0 {
        0
    } else {
        dos::err(if date {
            b"Invalid date\r\n"
        } else {
            b"Invalid time\r\n"
        });
        1
    }
}
