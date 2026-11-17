//! Native DOS regression. Install this executable as PSPJFT.EXE on the current drive.
#![no_std]
#![no_main]
use dos_tools::text;
use dosrs_realmode::{
    byte, store,
    system::{sw, w},
};

unsafe extern "Rust" {
    fn dos_memory_call(regs: *mut u16);
    fn dos_data_segment() -> u16;
}

fn dos(ax: u16, bx: u16, cx: u16, dx: u16, si: u16, es: u16) -> (u16, u16, bool) {
    let mut regs = [0u16; 12];
    regs[0] = ax;
    regs[1] = bx;
    regs[2] = cx;
    regs[3] = dx;
    regs[4] = si;
    regs[6] = es;
    regs[9] = 3;
    unsafe { dos_memory_call(regs.as_mut_ptr()) };
    (regs[0], regs[1], regs[10] & 1 == 0)
}
fn current() -> u16 {
    dos(0x6200, 0, 0, 0, 0, 0).1
}
fn close(fd: u16) -> bool {
    dos(0x3e00, fd, 0, 0, 0, 0).2
}
fn read(fd: u16, wanted: u8) -> bool {
    let mut value = 0u8;
    let at = (&mut value as *mut u8) as usize as u16;
    let (count, _, ok) = dos(0x3f00, fd, 1, at, 0, 0);
    ok && count == 1 && value == wanted
}
fn check() -> u16 {
    let parent = current();
    let segment = unsafe { dos_data_segment() };
    if !dos(0x4a00, segment - parent + 1024, 0, 0, 0, parent).2 {
        return 1;
    }
    let (child, _, allocated) = dos(0x4800, 0x20, 0, 0, 0, 0);
    if !allocated {
        return 2;
    }
    let file = *b"PSPJFT.EXE\0";
    let name = file.as_ptr() as usize as u16;
    let (fd, _, opened) = dos(0x3d00, 0, 0, name, 0, 0);
    let (private, _, hidden) = dos(0x3d80, 0, 0, name, 0, 0);
    if !opened || !hidden || fd >= 20 || private >= 20 {
        return 3;
    }
    // Model a Windows DOS task: the directory at PSP:18h no longer owns the JFT.
    let mut table = [0xffu8; 32];
    let address = table.as_mut_ptr() as usize as u16;
    let old = w(parent, 0x34);
    let old_seg = w(parent, 0x36);
    let old_count = w(parent, 0x32);
    let mut i = 0;
    while i < 20 {
        table[i as usize] = byte(old_seg, old + i) as u8;
        i += 1;
    }
    sw(parent, 0x34, address);
    sw(parent, 0x36, segment);
    sw(parent, 0x32, 32);
    store(old_seg, old + fd, 0xff);
    let mut result = 0;
    if !dos(0x4600, fd, 24, 0, 0, 0).2 || !read(24, b'M') {
        result = 4;
    } else {
        let top = child + 0x20;
        dos(0x5500, 0, 0, child, top, 0);
        if current() != child
            || w(child, 0) != 0x20cd
            || w(child, 2) != top
            || w(child, 0x16) != parent
        {
            result = 5;
        } else if w(child, 0x32) != 20 || w(child, 0x36) != child || w(child, 0x34) != 0x18 {
            result = 6;
        } else if byte(child, 0x18 + private) != 0xff || !read(fd, b'Z') || !close(fd) {
            result = 7;
        } else {
            let (local, _, ok) = dos(0x3d00, 0, 0, name, 0, 0);
            if !ok || !read(local, b'M') || !close(local) {
                result = 8;
            }
        }
        if current() == child {
            let mut n = 0;
            while n < 20 {
                if byte(child, 0x18 + n) != 255 {
                    close(n);
                }
                n += 1;
            }
        }
        dos(0x5000, parent, 0, 0, 0, 0);
        if result == 0
            && (!dos(0x4200, fd, 0, 0, 0, 0).2
                || !read(24, b'M')
                || !read(fd, b'Z')
                || !read(private, b'M'))
        {
            result = 9;
        }
        close(24);
    }
    // Restore both the PSP pointer and the original inline entry before exit.
    store(old_seg, old + fd, byte(segment, address + fd));
    sw(parent, 0x32, old_count);
    sw(parent, 0x34, old);
    sw(parent, 0x36, old_seg);
    close(fd);
    close(private);
    dos(0x4900, 0, 0, 0, 0, child);
    result
}
#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    let result = check();
    if result == 0 {
        text("PASS PSP/JFT\r\n");
    } else {
        text("FAIL PSP/JFT ");
        dos_tools::put(48 + result);
        text("\r\n");
    }
    result
}
