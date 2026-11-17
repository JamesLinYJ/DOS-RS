//! Exercise a one-byte echo character driver using DOS process APIs only.
#![no_std]
#![no_main]
use dos_tools::text;
unsafe extern "Rust" {
    fn dos_memory_call(r: *mut u16);
}

fn dos(ax: u16, bx: u16, cx: u16, dx: u16) -> (u16, u16, bool) {
    let mut r = [0u16; 12];
    r[0] = ax;
    r[1] = bx;
    r[2] = cx;
    r[3] = dx;
    r[9] = 3;
    unsafe {
        dos_memory_call(r.as_mut_ptr());
    }
    (r[0], r[3], r[10] & 1 == 0)
}
fn check() -> bool {
    let con = *b"CON\0";
    let mut handles = [0u16; 10];
    let mut i = 0;
    while i < 10 {
        let (fd, _, ok) = dos(0x3d02, 0, 0, con.as_ptr() as usize as u16);
        if !ok || (i != 0 && fd == handles[i - 1]) {
            text("CON open failed\r\n");
            return false;
        }
        handles[i] = fd;
        let (_, flags, ok) = dos(0x4400, fd, 0, 0);
        let (ready, _, status) = dos(0x4407, fd, 0, 0);
        if !ok || flags != 0x80d3 || !status || ready != 255 {
            text("CON status failed\r\n");
            return false;
        }
        i += 1;
    }
    while i > 0 {
        i -= 1;
        if !dos(0x3e00, handles[i], 0, 0).2 {
            return false;
        }
    }
    text("PASS: ten independent CON handles and output-ready status\r\n");
    let name = *b"echobox\0";
    let name_off = name.as_ptr() as usize as u16;
    let (fd, _, ok) = dos(0x3d02, 0, 0, name_off);
    if !ok {
        text("open failed\r\n");
        return false;
    }
    let (_, flags, ok) = dos(0x4400, fd, 0, 0);
    if !ok || flags & 0x80 == 0 {
        text("device flags failed\r\n");
        return false;
    }
    let mut payload = [0x51u8, 0x7b];
    let source = payload.as_mut_ptr() as usize as u16;
    let (actual, _, ok) = dos(0x4000, fd, 1, source);
    if !ok || actual != 1 {
        text("device write failed\r\n");
        return false;
    }
    let (actual, _, ok) = dos(0x3f00, fd, 1, source + 1);
    if !ok || actual != 1 || payload[1] != 0x51 {
        text("device read failed\r\n");
        return false;
    }
    if dos(0x3f00, fd, 2, 0xffff).2 {
        text("wrapped buffer accepted\r\n");
        return false;
    }
    if flags & 0x4000 != 0 {
        payload[0] = 0xa6;
        if !dos(0x4403, fd, 1, source).2 || !dos(0x4402, fd, 1, source + 1).2 || payload[1] != 0xa6
        {
            text("IOCTL transfer failed\r\n");
            return false;
        }
        if dos(0x4407, fd, 0, 0).0 != 255 || dos(0x4406, fd, 0, 0).0 != 0 {
            text("ready/busy failed\r\n");
            return false;
        }
    } else if dos(0x4403, fd, 1, source).2 || dos(0x4402, fd, 1, source).2 {
        text("unsupported IOCTL accepted\r\n");
        return false;
    }
    if !dos(0x3e00, fd, 0, 0).2 || dos(0x3f00, fd, 1, source).2 || dos(0x4400, fd, 0, 0).2 {
        text("stale handle accepted\r\n");
        return false;
    }
    let (fd, _, ok) = dos(0x3d00, 0, 0, name_off);
    ok && !dos(0x4000, fd, 1, source).2 && dos(0x3e00, fd, 0, 0).2
}
#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    if check() {
        text("PASS: DOS character device I/O\r\n");
        0
    } else {
        text("FAIL: DOS character device I/O\r\n");
        1
    }
}
