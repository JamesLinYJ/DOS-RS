//! DOS command tails are byte strings in the active code page.
pub const LIMIT: usize = 126;

pub fn upper(c: u8) -> u8 {
    if c >= b'a' && c <= b'z' { c - 32 } else { c }
}

pub fn equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if upper(a[i]) != upper(b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

pub fn space(c: u8) -> bool {
    c == b' ' || c == b'\t'
}
pub fn left(mut data: &[u8]) -> &[u8] {
    while !data.is_empty() && space(data[0]) {
        data = rest(&data, 1);
    }
    data
}
pub fn trim(data: &[u8]) -> &[u8] {
    let data = left(data);
    let mut n = data.len();
    while n > 0 && space(data[n - 1]) {
        n -= 1;
    }
    head(&data, n)
}

pub fn word(data: &[u8]) -> (&[u8], &[u8]) {
    let data = left(data);
    let mut i = 0;
    let mut quote = false;
    while i < data.len() {
        if data[i] == b'"' {
            quote = !quote;
        }
        if !quote && space(data[i]) {
            break;
        }
        i += 1;
    }
    (head(&data, i), left(rest(&data, i)))
}
pub fn unquote(data: &[u8]) -> &[u8] {
    if data.len() >= 2 && data[0] == b'"' && data[data.len() - 1] == b'"' {
        part(data, 1, data.len() - 1)
    } else {
        data
    }
}
pub fn command(data: &[u8]) -> (&[u8], &[u8]) {
    let data = left(data);
    let (name, args) = word(data);
    let mut i = 0;
    while i < name.len() {
        let c = name[i];
        if c == b'/'
            || ((c == b'=' || c == b';') && equal(head(name, i), b"PATH"))
            || ((c == b'.' || c == b'\\')
                && (equal(head(&name, i), b"CD")
                    || equal(head(&name, i), b"CHDIR")
                    || equal(head(&name, i), b"ECHO")))
        {
            return (head(&data, i), rest(&data, i));
        }
        i += 1;
    }
    (unquote(name), args)
}
pub fn push(buf: &mut [u8], n: &mut usize, data: &[u8]) -> bool {
    if data.len() > buf.len() - *n {
        return false;
    }
    let mut i = 0;
    while i < data.len() {
        buf[*n] = data[i];
        *n += 1;
        i += 1;
    }
    true
}
pub fn split_at(data: &[u8], delim: u8) -> usize {
    let mut at = 0;
    let mut quote = false;
    while at < data.len() {
        if data[at] == b'"' {
            quote = !quote;
        }
        if !quote && data[at] == delim {
            break;
        }
        at += 1;
    }
    at
}
pub fn contains(s: &[u8], c: u8) -> bool {
    let mut i = 0;
    while i < s.len() {
        if s[i] == c {
            return true;
        }
        i += 1;
    }
    false
}
pub fn part(data: &[u8], start: usize, end: usize) -> &[u8] {
    data.get(start..end).unwrap_or(&[])
}
pub fn rest(data: &[u8], start: usize) -> &[u8] {
    part(data, start, data.len())
}
pub fn head(data: &[u8], end: usize) -> &[u8] {
    part(data, 0, end)
}
pub fn exact(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}
