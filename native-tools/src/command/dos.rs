//! Near-buffer DOS INT 21h calls; far ownership is confined to the process PSP.
use super::parse::{LIMIT, unquote};
use dosrs_realmode::{byte, system::w};

unsafe extern "Rust" {
    fn dos_memory_call(regs: *mut u16);
    fn dos_data_segment() -> u16;
}
pub fn ptr<T>(p: *const T) -> u16 {
    p as usize as u16
}
pub fn ds() -> u16 {
    unsafe { dos_data_segment() }
}
pub fn call(r: &mut [u16; 12]) -> bool {
    r[9] = 3;
    unsafe {
        dos_memory_call(r.as_mut_ptr());
    }
    r[10] & 1 == 0
}
pub fn service(ax: u16, bx: u16, cx: u16, dx: u16) -> Result<u16, u16> {
    let mut r = [0; 12];
    r[0] = ax;
    r[1] = bx;
    r[2] = cx;
    r[3] = dx;
    if call(&mut r) { Ok(r[0]) } else { Err(r[0]) }
}
pub fn pathbuf(path: &[u8], buf: &mut [u8; 128]) -> bool {
    let name = unquote(path);
    if name.is_empty() || name.len() >= buf.len() {
        return false;
    }
    let mut i = 0;
    while i < name.len() {
        if name[i] == 0 {
            return false;
        }
        buf[i] = name[i];
        i += 1;
    }
    buf[i] = 0;
    true
}
pub fn path_call(ax: u16, name: &[u8], attr: u16) -> Result<u16, u16> {
    let mut buf = [0; 128];
    if !pathbuf(name, &mut buf) {
        return Err(3);
    }
    service(ax, 0, attr, ptr(buf.as_ptr()))
}
pub fn open(name: &[u8], mode: u16) -> Result<u16, u16> {
    path_call(0x3d00 | mode, name, 0)
}
pub fn create(name: &[u8]) -> Result<u16, u16> {
    path_call(0x3c00, name, 0)
}
pub fn close(fd: u16) -> bool {
    service(0x3e00, fd, 0, 0).is_ok()
}
pub fn read(fd: u16, buf: &mut [u8]) -> Result<u16, u16> {
    service(0x3f00, fd, buf.len() as u16, ptr(buf.as_mut_ptr()))
}
pub fn write(fd: u16, buf: &[u8]) -> Result<u16, u16> {
    service(0x4000, fd, buf.len() as u16, ptr(buf.as_ptr()))
}
pub fn output(fd: u16, mut buf: &[u8]) {
    while !buf.is_empty() {
        match write(fd, buf) {
            Ok(n) if n != 0 => buf = super::parse::rest(&buf, n as usize),
            _ => return,
        }
    }
}
pub fn out(buf: &[u8]) {
    output(1, buf)
}
pub fn err(buf: &[u8]) {
    output(2, buf)
}
pub fn console(fd: u16) -> bool {
    let mut r = [0; 12];
    r[0] = 0x4400;
    r[1] = fd;
    call(&mut r) && r[3] & 0x80 != 0
}
pub fn drive() -> u16 {
    service(0x1900, 0, 0, 0).unwrap_or(0) & 255
}
pub fn cwd(drive: u16, buf: &mut [u8]) -> usize {
    if buf.len() < 64 {
        return 0;
    }
    let mut r = [0; 12];
    r[0] = 0x4700;
    r[3] = drive + 1;
    r[4] = ptr(buf.as_mut_ptr());
    if !call(&mut r) {
        return 0;
    }
    let mut i = 0;
    while i < buf.len() && buf[i] != 0 {
        i += 1;
    }
    i
}
pub fn psp() -> u16 {
    let mut r = [0; 12];
    r[0] = 0x6200;
    call(&mut r);
    r[1]
}
pub fn shrink(psp: u16) -> bool {
    let segment = ds();
    // A DOS-RS shell may have a separately owned stack below its PSP.
    if segment < psp || w(psp, 2) <= segment + 1024 {
        return true;
    }
    let mut r = [0; 12];
    r[0] = 0x4a00;
    r[1] = segment - psp + 1024;
    r[6] = psp;
    call(&mut r)
}
pub fn seek(fd: u16, origin: u16, pos: u32) -> Result<u32, u16> {
    let mut r = [0; 12];
    r[0] = 0x4200 | origin;
    r[1] = fd;
    r[2] = (pos >> 16) as u16;
    r[3] = pos as u16;
    if call(&mut r) {
        Ok((u32::from(r[3]) << 16) | u32::from(r[0]))
    } else {
        Err(r[0])
    }
}
pub fn exists(name: &[u8]) -> bool {
    let mut buf = [0; 128];
    if !pathbuf(name, &mut buf) {
        return false;
    }
    let mut r = [0; 12];
    r[0] = 0x4300;
    r[3] = ptr(buf.as_ptr());
    call(&mut r)
}
pub fn remove(name: &[u8]) {
    let _ = path_call(0x4100, name, 0);
}
pub fn tail(psp: u16, out: &mut [u8]) -> usize {
    let mut n = usize::from(byte(psp, 0x80));
    if n > LIMIT {
        n = LIMIT;
    }
    let mut i = 0;
    while i < n {
        out[i] = byte(psp, 0x81 + i as u16) as u8;
        i += 1;
    }
    n
}
fn fcb(field: &[u8], buf: &mut [u8; 37]) {
    let mut i = 1;
    while i <= 11 {
        buf[i] = b' ';
        i += 1;
    }
    let name = unquote(field);
    let mut start = 0;
    if name.len() > 1 && name[1] == b':' {
        let c = super::parse::upper(name[0]);
        if c >= 65 && c <= 90 {
            buf[0] = c - 64;
        }
        start = 2;
    }
    i = start;
    while i < name.len() {
        if name[i] == b'\\' {
            start = i + 1;
        }
        i += 1;
    }
    let mut at = 1;
    i = start;
    while i < name.len() && at < 12 {
        let c = name[i];
        if c == b'.' {
            at = 9;
        } else if at < 9 || super::parse::contains(super::parse::part(&name, start, i), b'.') {
            buf[at] = super::parse::upper(c);
            at += 1;
        }
        i += 1;
    }
}
pub fn exec(name: &[u8], args: &[u8]) -> Result<u16, u16> {
    let mut path = [0u8; 128];
    if !pathbuf(name, &mut path) {
        return Err(3);
    }
    let mut tail = [0u8; 128];
    let mut n = 0;
    if !args.is_empty() {
        if args.len() >= LIMIT {
            return Err(10);
        }
        tail[1] = b' ';
        n = 1;
        let mut i = 0;
        while i < args.len() {
            tail[n + 1] = args[i];
            i += 1;
            n += 1;
        }
    }
    tail[0] = n as u8;
    tail[n + 1] = 13;
    let (first, rest) = super::parse::word(args);
    let (second, _) = super::parse::word(rest);
    let mut a = [0u8; 37];
    let mut b = [0u8; 37];
    fcb(first, &mut a);
    fcb(second, &mut b);
    let seg = ds();
    let params = [
        0u16,
        ptr(tail.as_ptr()),
        seg,
        ptr(a.as_ptr()),
        seg,
        ptr(b.as_ptr()),
        seg,
    ];
    let mut r = [0; 12];
    r[0] = 0x4b00;
    r[1] = ptr(params.as_ptr());
    r[3] = ptr(path.as_ptr());
    r[6] = seg;
    if call(&mut r) {
        Ok(service(0x4d00, 0, 0, 0).unwrap_or(1) & 255)
    } else {
        Err(r[0])
    }
}
pub fn inherited_env(psp: u16) -> u16 {
    w(psp, 0x2c)
}
pub fn get_dta() -> (u16, u16) {
    let mut r = [0; 12];
    r[0] = 0x2f00;
    call(&mut r);
    (r[6], r[1])
}
pub fn set_dta(seg: u16, off: u16) {
    unsafe extern "Rust" {
        fn edit_far_io(fd: u16, seg: u16, off: u16, n: u16, ax: u16) -> u16;
    }
    unsafe {
        edit_far_io(0, seg, off, 0, 0x1a00);
    }
}
