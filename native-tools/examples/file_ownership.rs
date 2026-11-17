//! DOS process and file API regression client. The host independently inspects the disk.
#![no_std]
#![no_main]
use dos_tools::{argument, text};

unsafe extern "Rust" {
    fn dos_memory_call(regs: *mut u16);
    fn dos_data_segment() -> u16;
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
