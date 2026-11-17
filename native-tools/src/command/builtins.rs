//! COMMAND.COM built-ins. All filesystem and process state goes through INT 21h.
use super::{dos, parse};
#[path = "clock.rs"]
mod clock;
#[path = "directory.rs"]
mod directory;
#[path = "files.rs"]
mod files;
use parse::{equal, upper};

pub(super) fn message(error: u16) -> u16 {
    let s: &[u8] = match error {
        2 | 18 => b"File not found",
        3 => b"Invalid path",
        5 => b"Access denied",
        15 => b"Invalid drive specification",
        19 => b"Write protect error",
        28 => b"Printer out of paper",
        39 | 112 => b"Insufficient disk space",
        87 => b"Invalid parameter",
        _ => b"DOS error",
    };
    dos::err(s);
    dos::err(b"\r\n");
    1
}
pub(super) fn syntax() -> u16 {
    dos::err(b"Syntax error\r\n");
    1
}
pub(super) fn result(value: Result<u16, u16>) -> u16 {
    match value {
        Ok(_) => 0,
        Err(e) => message(e),
    }
}
pub(super) fn number(value: u32, pad: usize) {
    // 286 has a 16-bit divide. Divide a pair of words by ten in base 256.
    let mut digits = [b' '; 12];
    let mut pos = 12;
    let mut n = value;
    loop {
        let mut q = 0;
        let mut r = 0u16;
        let mut shift = 32;
        while shift > 0 {
            shift -= 8;
            r = r * 256 + ((n >> shift) & 255) as u16;
            q = (q << 8) | u32::from(r / 10);
            r %= 10;
        }
        pos -= 1;
        digits[pos] = b'0' + r as u8;
        n = q;
        if n == 0 {
            break;
        }
    }
    if pad > 12 - pos && pad <= 12 {
        pos = 12 - pad;
    }
    dos::out(parse::rest(&digits, pos));
}
pub(super) fn byte(c: u8) {
    dos::out(&[c]);
}
pub(super) fn newline() {
    dos::out(b"\r\n");
}
pub(super) fn two(v: u16) {
    byte(b'0' + (v / 10 % 10) as u8);
    byte(b'0' + (v % 10) as u8);
}

#[derive(Clone, Copy)]
pub(super) struct Path {
    buf: [u8; 128],
    n: usize,
}
impl Path {
    fn empty() -> Self {
        Self {
            buf: [0; 128],
            n: 0,
        }
    }
    fn from(b: &[u8]) -> Result<Self, u16> {
        let mut s = Self::empty();
        s.push(parse::unquote(b))?;
        Ok(s)
    }
    fn push(&mut self, b: &[u8]) -> Result<(), u16> {
        if self.n + b.len() >= self.buf.len() {
            return Err(3);
        }
        if !parse::push(&mut self.buf, &mut self.n, b) {
            return Err(3);
        }
        self.buf[self.n] = 0;
        Ok(())
    }
    fn bytes(&self) -> &[u8] {
        parse::head(&self.buf, self.n)
    }
    fn component(&self) -> usize {
        let mut i = self.n;
        while i > 0 {
            if matches!(self.buf[i - 1], b':' | b'\\') {
                break;
            }
            i -= 1;
        }
        i
    }
    fn trim(&mut self, n: usize) {
        self.n = n;
        self.buf[n] = 0;
    }
    fn join(&mut self, b: &[u8]) -> Result<(), u16> {
        if self.n != 0 && !matches!(self.buf[self.n - 1], b'\\' | b':') {
            self.push(b"\\")?;
        }
        self.push(b)
    }
    fn attribute(&self) -> Result<u16, u16> {
        let mut r = [0; 12];
        r[0] = 0x4300;
        r[3] = dos::ptr(self.buf.as_ptr());
        if dos::call(&mut r) {
            Ok(r[2])
        } else {
            Err(r[0])
        }
    }
    fn true_name(&self) -> Result<Self, u16> {
        let mut dst = Self::empty();
        let mut r = [0; 12];
        r[0] = 0x6000;
        r[4] = dos::ptr(self.buf.as_ptr());
        r[5] = dos::ptr(dst.buf.as_mut_ptr());
        r[6] = dos::ds();
        if !dos::call(&mut r) {
            return Err(r[0]);
        }
        while dst.n < 127 && dst.buf[dst.n] != 0 {
            dst.n += 1;
        }
        Ok(dst)
    }
}
struct Search {
    data: [u8; 43],
    started: bool,
}
impl Search {
    fn new() -> Self {
        Self {
            data: [0; 43],
            started: false,
        }
    }
    fn next(&mut self, path: &Path, attr: u16) -> Result<bool, u16> {
        let old = dos::get_dta();
        dos::set_dta(dos::ds(), dos::ptr(self.data.as_mut_ptr()));
        let mut r = [0; 12];
        r[0] = if self.started { 0x4f00 } else { 0x4e00 };
        r[2] = attr;
        r[3] = dos::ptr(path.buf.as_ptr());
        self.started = true;
        let ok = dos::call(&mut r);
        dos::set_dta(old.0, old.1);
        if ok {
            Ok(true)
        } else if matches!(r[0], 2 | 18) {
            Ok(false)
        } else {
            Err(r[0])
        }
    }
    fn name(&self) -> &[u8] {
        let mut n = 30;
        while n < 43 && self.data[n] != 0 {
            n += 1;
        }
        parse::part(&self.data, 30, n)
    }
    fn attr(&self) -> u8 {
        self.data[21]
    }
    fn word(&self, off: usize) -> u16 {
        u16::from(self.data[off]) | (u16::from(self.data[off + 1]) << 8)
    }
    fn size(&self) -> u32 {
        u32::from(self.word(26)) | (u32::from(self.word(28)) << 16)
    }
}
fn wildcard(b: &[u8]) -> bool {
    let mut i = 0;
    while i < b.len() {
        if matches!(b[i], b'*' | b'?') {
            return true;
        }
        i += 1;
    }
    false
}
fn toggle(name: &[u8], args: &[u8], verify: bool) -> u16 {
    let mut r = [0; 12];
    if args.is_empty() {
        r[0] = if verify { 0x5400 } else { 0x3300 };
        if !dos::call(&mut r) {
            return message(r[0]);
        }
        dos::out(name);
        dos::out(if (if verify { r[0] } else { r[3] }) & 1 != 0 {
            b" is on\r\n"
        } else {
            b" is off\r\n"
        });
        return 0;
    }
    let on = equal(args, b"ON");
    if !on && !equal(args, b"OFF") {
        return syntax();
    }
    r[0] = if verify {
        0x2e00 + u16::from(on)
    } else {
        0x3301
    };
    r[3] = if verify { 0 } else { u16::from(on) };
    if dos::call(&mut r) { 0 } else { message(r[0]) }
}
fn ctty(args: &[u8]) -> Result<u16, u16> {
    let fd = dos::open(args, 2)?;
    let mut r = [0; 12];
    r[0] = 0x4400;
    r[1] = fd;
    let v = if !dos::call(&mut r) || r[3] & 0x80 == 0 {
        Err(5)
    } else {
        let mut i = 0;
        let mut e = Ok(0);
        while i < 3 {
            if let Err(n) = dos::service(0x4600, fd, i, 0) {
                e = Err(n);
                break;
            }
            i += 1;
        }
        e
    };
    dos::close(fd);
    v
}
fn hex16(n: u16) {
    let mut shift = 16;
    while shift != 0 {
        shift -= 4;
        byte(b"0123456789ABCDEF"[((n >> shift) & 15) as usize]);
    }
}
fn drive_num(arg: &[u8]) -> Result<u16, u16> {
    if arg.is_empty() {
        return Ok(dos::drive());
    }
    if arg.len() == 2 && arg[1] == b':' {
        let c = upper(arg[0]);
        if c >= b'A' && c <= b'Z' {
            return Ok(u16::from(c - b'A'));
        }
    }
    Err(15)
}
fn vol(arg: &[u8]) -> Result<u16, u16> {
    let drive = drive_num(arg)?;
    let mut buf = [0; 25];
    let mut r = [0; 12];
    r[0] = 0x6900;
    r[1] = drive + 1;
    r[3] = dos::ptr(buf.as_mut_ptr());
    if !dos::call(&mut r) {
        return Err(r[0]);
    }
    dos::out(b"\r\n Volume in drive ");
    byte(b'A' + drive as u8);
    let label = parse::part(&buf, 6, 17);
    if equal(label, b"NO NAME    ") || equal(label, b"           ") {
        dos::out(b" has no label");
    } else {
        dos::out(b" is ");
        dos::out(parse::part(&buf, 6, 17));
    }
    dos::out(b"\r\n Volume Serial Number is ");
    hex16(u16::from(buf[4]) | (u16::from(buf[5]) << 8));
    byte(b'-');
    hex16(u16::from(buf[2]) | (u16::from(buf[3]) << 8));
    newline();
    Ok(0)
}
fn cd(args: &[u8]) -> u16 {
    if args.is_empty() || args.len() == 2 && args[1] == b':' {
        let drive = match drive_num(args) {
            Ok(d) => d,
            Err(e) => return message(e),
        };
        let mut path = [0; 64];
        let n = dos::cwd(drive, &mut path);
        byte(b'A' + drive as u8);
        dos::out(b":\\");
        dos::out(parse::head(&path, n));
        newline();
        0
    } else {
        result(dos::path_call(0x3b00, args, 0))
    }
}

pub fn run(name: &[u8], args: &[u8], batch: bool) -> Option<u16> {
    let a = parse::trim(args);
    Some(if equal(name, b"DIR") {
        result(directory::dir(a))
    } else if equal(name, b"CD") || equal(name, b"CHDIR") {
        cd(a)
    } else if equal(name, b"MD") || equal(name, b"MKDIR") {
        if a.is_empty() {
            syntax()
        } else {
            result(dos::path_call(0x3900, a, 0))
        }
    } else if equal(name, b"RD") || equal(name, b"RMDIR") {
        if a.is_empty() {
            syntax()
        } else {
            result(dos::path_call(0x3a00, a, 0))
        }
    } else if equal(name, b"TYPE") {
        result(files::display(a))
    } else if equal(name, b"REN") || equal(name, b"RENAME") {
        result(files::rename(a))
    } else if equal(name, b"DEL") || equal(name, b"ERASE") {
        result(files::delete(a))
    } else if equal(name, b"COPY") {
        result(files::copy(a, batch))
    } else if equal(name, b"VOL") {
        result(vol(a))
    } else if equal(name, b"BREAK") {
        toggle(b"BREAK", a, false)
    } else if equal(name, b"VERIFY") {
        toggle(b"VERIFY", a, true)
    } else if equal(name, b"DATE") || equal(name, b"TIME") {
        clock::run(equal(name, b"DATE"), a)
    } else if equal(name, b"CHCP") {
        code_page(a)
    } else if equal(name, b"CTTY") {
        result(ctty(a))
    } else if equal(name, b"CLS") {
        dos_tools::screen(2, 0, 0);
        0
    } else if equal(name, b"TRUENAME") {
        match Path::from(if a.is_empty() { b"." } else { a }).and_then(|p| p.true_name()) {
            Ok(p) => {
                newline();
                dos::out(p.bytes());
                newline();
                0
            }
            Err(e) => message(e),
        }
    } else if equal(name, b"VER") {
        let version = dos::service(0x3000, 0, 0, 0).unwrap_or(0);
        dos::out(b"\r\nDOS Version ");
        number(u32::from(version & 255), 0);
        byte(b'.');
        two(version >> 8);
        dos::out(b"\r\n\r\n");
        0
    } else {
        return None;
    })
}

/// Short usage for the internal commands, including the alternate DOS spellings.
pub fn help(name: &[u8]) -> bool {
    let usage=b"BREAK [ON|OFF]\nCALL [drive:][path]batch [arguments]\nCD [drive:][path]\nCHDIR [drive:][path]\nCHCP [code-page]\nCLS\nCOPY [/A|/B] source[+source...] [destination] [/A|/B] [/V] [/Y|/-Y]\nCTTY device\nDATE [mm-dd-yy]\nDEL [drive:][path]name [/P]\nERASE [drive:][path]name [/P]\nDIR [drive:][path][pattern] [/P] [/W] [/A[:attributes]] [/O[:order]] [/S] [/B] [/L]\nECHO [ON|OFF|message]\nEXIT\nFOR %variable IN (set) DO command\nGOTO label\nIF [NOT] condition command\nLH [options] program [arguments]\nLOADHIGH [options] program [arguments]\nMD [drive:]path\nMKDIR [drive:]path\nPATH [directories]\nPAUSE\nPROMPT [text]\nRD [drive:]path\nRMDIR [drive:]path\nREM [comment]\nREN [drive:][path]oldname newname\nRENAME [drive:][path]oldname newname\nSET [variable=[value]]\nSHIFT\nTIME [hh:mm[:ss[.cc]]]\nTYPE [drive:][path]filename\nVER\nVERIFY [ON|OFF]\nVOL [drive:]\nTRUENAME [drive:][path]name\n";
    let mut start = 0;
    while start < usage.len() {
        let mut end = start;
        let mut n = 0;
        while end < usage.len() && usage[end] != 10 {
            if n == 0 && usage[end] == 32 {
                n = end - start;
            }
            end += 1;
        }
        if n == 0 {
            n = end - start;
        }
        if equal(name, parse::part(usage, start, start + n)) {
            dos::out(parse::part(usage, start, end));
            newline();
            return true;
        }
        start = end + 1;
    }
    false
}
fn code_page(args: &[u8]) -> u16 {
    let mut r = [0; 12];
    if args.is_empty() {
        r[0] = 0x6601;
    } else {
        let mut value = 0u16;
        let mut i = 0;
        while i < args.len() {
            let digit = args[i];
            if digit < b'0' || digit > b'9' || value > 6553 || value == 6553 && digit > b'5' {
                return syntax();
            }
            value = value * 10 + u16::from(digit - b'0');
            i += 1;
        }
        r[0] = 0x6602;
        r[1] = value;
    }
    if !dos::call(&mut r) {
        dos::err(b"Invalid code page\r\n");
        return 1;
    }
    if args.is_empty() {
        dos::out(b"Active code page: ");
        number(u32::from(r[1]), 0);
        newline();
    }
    0
}
