//! DOS process and file API regression client. The host independently inspects the disk.
#![no_std]
#![no_main]
use dos_tools::{argument, text};

unsafe extern "Rust" {
    fn dos_memory_call(regs: *mut u16);
    fn dos_data_segment() -> u16;
    fn arg(index: u16, at: u16) -> u16;
}

fn call(ax: u16, bx: u16, cx: u16, dx: u16, es: u16) -> (u16, u16, bool) {
    let mut r = [0u16; 12];
    r[0] = ax;
    r[1] = bx;
    r[2] = cx;
    r[3] = dx;
    r[6] = es;
    r[9] = 3;
    unsafe { dos_memory_call(r.as_mut_ptr()) };
    (r[0], if ax == 0x6200 { r[1] } else { r[3] }, r[10] & 1 == 0)
}

fn ptr<T>(p: *const T) -> u16 {
    p as usize as u16
}

fn close(fd: u16) -> bool {
    call(0x3e00, fd, 0, 0, 0).2
}

fn child(path: &[u8], tail: &[u8]) -> bool {
    let ds = unsafe { dos_data_segment() };
    let psp = call(0x6200, 0, 0, 0, 0).1;
    // Rust16 MZ files place the data segment and the 16 KiB stack after the code.
    if !call(0x4a00, ds - psp + 1024, 0, 0, psp).2 {
        text("OWN shrink failed\r\n");
        return false;
    }
    let params = [0u16, ptr(tail.as_ptr()), ds, 0x5c, psp, 0x6c, psp];
    call(0x4b00, ptr(params.as_ptr()), 0, ptr(path.as_ptr()), ds).2
}

fn trial(name: &[u8], program: &[u8], tail: &[u8]) -> bool {
    let payload = *b"0123456789ABCDEF";
    let r = call(0x3c00, 0, 0, ptr(name.as_ptr()), 0);
    if !r.2 {
        return false;
    }
    let fd = r.0;
    let r = call(0x4000, fd, 16, ptr(payload.as_ptr()), 0);
    if !r.2 || r.0 != 16 {
        return false;
    }
    let executed = program.is_empty() || child(program, tail);
    let closed = close(fd);
    executed && closed
}

fn persisted(name: &[u8]) -> bool {
    let r = call(0x3d00, 0, 0, ptr(name.as_ptr()), 0);
    if !r.2 {
        return false;
    }
    let mut bytes = [0u8; 17];
    let fd = r.0;
    let r = call(0x3f00, fd, 17, ptr(bytes.as_mut_ptr()), 0);
    if !close(fd) || !r.2 || r.0 != 16 {
        return false;
    }
    let expected = *b"0123456789ABCDEF";
    let mut i = 0;
    while i < 16 {
        if bytes[i] != expected[i] {
            return false;
        }
        i += 1;
    }
    true
}

fn read_byte(fd: u16, expected: u8) -> bool {
    let mut byte = 0u8;
    let (count, _, ok) = call(0x3f00, fd, 1, ptr(&mut byte), 0);
    ok && count == 1 && byte == expected
}

fn inherited() -> bool {
    let dup = unsafe { arg(2, 0) };
    let independent = unsafe { arg(3, 0) };
    if !(65..85).contains(&dup) || !(65..85).contains(&independent) {
        return false;
    }
    read_byte(dup - 65, b'3')
        && close(dup - 65)
        && read_byte(independent - 65, b'1')
        && close(independent - 65)
}

fn handles() -> bool {
    let name = *b"SEQ.BIN\0";
    let path = ptr(name.as_ptr());
    let (a, _, ok_a) = call(0x3d00, 0, 0, path, 0);
    let (b, _, ok_b) = call(0x3d00, 0, 0, path, 0);
    let (dest, _, ok_dest) = call(0x3d00, 0, 0, path, 0);
    if !ok_a || !ok_b || !ok_dest || !read_byte(a, b'0') || !read_byte(b, b'0') {
        text("OWN: independent open failed\r\n");
        return false;
    }
    let (dup, _, ok) = call(0x4500, a, 0, 0, 0);
    if !ok
        || dup >= 20
        || b >= 20
        || !read_byte(dup, b'1')
        || !call(0x4600, a, dest, 0, 0).2
        || !read_byte(dest, b'2')
    {
        text("OWN: duplicate failed\r\n");
        return false;
    }
    let mut tail = *b"\x0c INHERIT A B\r";
    tail[10] = b'A' + dup as u8;
    tail[12] = b'A' + b as u8;
    let exe = *b"OWNER.EXE\0";
    if !child(&exe, &tail) || call(0x4d00, 0, 0, 0, 0).0 != 0 {
        text("OWN: inherited handle failed\r\n");
        return false;
    }
    read_byte(a, b'4')
        && read_byte(b, b'2')
        && read_byte(dup, b'5')
        && close(a)
        && close(b)
        && close(dup)
        && close(dest)
}

fn seek(fd: u16, pos: u32) -> bool {
    let (low, high, ok) = call(0x4200, fd, (pos >> 16) as u16, pos as u16, 0);
    ok && low == pos as u16 && high == (pos >> 16) as u16
}

fn short_read() -> bool {
    let name = *b"SEQ.BIN\0";
    let (fd, _, opened) = call(0x3d00, 0, 0, ptr(name.as_ptr()), 0);
    if !opened || !seek(fd, 8) {
        return false;
    }
    let mut guard = [0x5au8; 4];
    let offset = ptr(guard.as_mut_ptr()) + 1;
    let (n, _, ok) = call(0x3f00, fd, 0xffff, offset, 0);
    if !ok || n != 2 || guard != [0x5a, b'8', b'9', 0x5a] {
        text("OWN: short EOF read failed\r\n");
        return false;
    }
    // Two bytes really remain. The failed transfer must leave the cursor alone.
    if !seek(fd, 8) || call(0x3f00, fd, 2, 0xffff, 0).2 {
        text("OWN: actual segment overflow accepted\r\n");
        return false;
    }
    read_byte(fd, b'8') && read_byte(fd, b'9') && close(fd)
}

fn pattern(block: u16, offset: u16) -> u8 {
    (block ^ (block >> 8) ^ offset ^ (offset >> 8)) as u8
}

fn stream() -> bool {
    let name = *b"STREAM.BIN\0";
    let (fd, _, opened) = call(0x3c00, 0, 0, ptr(name.as_ptr()), 0);
    if !opened {
        return false;
    }
    let mut buffer = [0u8; 512];
    let mut block = 0;
    while block < 262 {
        let mut i = 0;
        while i < 512 {
            buffer[i] = pattern(block, i as u16);
            i += 1;
        }
        let (n, _, ok) = call(0x4000, fd, 512, ptr(buffer.as_ptr()), 0);
        if !ok || n != 512 {
            return false;
        }
        block += 1;
    }
    if !seek(fd, 0x20017) {
        return false;
    }
    let (n, _, ok) = call(0x4000, fd, 0, ptr(buffer.as_ptr()), 0);
    if !ok || n != 0 || !close(fd) {
        return false;
    }
    true
}

fn read_stream() -> bool {
    let name = *b"STREAM.BIN\0";
    let (fd, _, opened) = call(0x3d00, 0, 0, ptr(name.as_ptr()), 0);
    if !opened {
        return false;
    }
    let (lo, hi, ok) = call(0x4202, fd, 0, 0, 0);
    if !ok || lo != 23 || hi != 2 {
        return false;
    }
    let mut boundary = 1u16;
    while boundary <= 2 {
        if !seek(fd, ((boundary as u32) << 16) - 1) {
            return false;
        }
        let block = boundary * 128;
        if !read_byte(fd, pattern(block - 1, 511)) || !read_byte(fd, pattern(block, 0)) {
            return false;
        }
        boundary += 1;
    }
    close(fd)
}

fn full_disk() -> bool {
    let name = *b"GUARD.BIN\0";
    let (fd, _, ok) = call(0x3d02, 0, 0, ptr(name.as_ptr()), 0);
    if !ok || !seek(fd, 1024) {
        return false;
    }
    let bytes = [0xacu8; 2048];
    let (n, _, written) = call(0x4000, fd, 2048, ptr(bytes.as_ptr()), 0);
    // One free 512-byte cluster was left on the volume.
    if !written || n != 512 {
        return false;
    }
    let (n, _, written) = call(0x4000, fd, 2048, ptr(bytes.as_ptr()), 0);
    written && n == 0 && close(fd)
}

#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    let control = *b"CTRL.BIN\0";
    let first = *b"EXECA.BIN\0";
    let second = *b"EXECB.BIN\0";
    let files = *b"FILES.COM\0";
    let owner = *b"OWNER.EXE\0";
    let empty = [0u8, 13];
    let child_tail = *b"\x04 KID\r";
    let success = if argument(1, "N") {
        trial(&control, &[], &empty)
    } else if argument(1, "A") {
        trial(&first, &files, &empty)
    } else if argument(1, "B") {
        trial(&second, &owner, &child_tail)
    } else if argument(1, "KID") {
        text("OWN: child ran\r\n");
        return 0;
    } else if argument(1, "INHERIT") {
        if inherited() {
            text("OWN: child closed inherited handles\r\n");
            return 0;
        }
        return 1;
    } else if argument(1, "HANDLES") {
        handles()
    } else if argument(1, "SHORT") {
        short_read()
    } else if argument(1, "STREAM") {
        stream()
    } else if argument(1, "LARGE") {
        read_stream()
    } else if argument(1, "FULL") {
        full_disk()
    } else if argument(1, "READ") {
        persisted(&control) && persisted(&first) && persisted(&second)
    } else {
        false
    };
    if success {
        text("OWN: OK\r\n");
        0
    } else {
        text("OWN: FAIL\r\n");
        1
    }
}
