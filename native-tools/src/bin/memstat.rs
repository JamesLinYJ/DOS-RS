//! Public DOS memory API diagnostic for the native installation disk.
#![no_std]
#![no_main]
use dos_tools::{argument, install::number, text};
use dosrs_realmode::{byte, store, system::w};
#[path = "memstat/high_memory.rs"]
mod high_memory;
unsafe extern "Rust" {
    fn dos_memory_call(regs: *mut u16);
    fn dos_data_segment() -> u16;
}
type Regs = [u16; 12];
#[path = "memstat/handle_table.rs"]
mod handle_table;
#[path = "memstat/import.rs"]
mod import;
const EMS: u16 = 0;
const MULTIPLEX: u16 = 1;
const XMS: u16 = 2;
const DOS: u16 = 3;
const VCPI: u16 = 4;

fn call(r: &mut Regs, api: u16, ax: u16, bx: u16, dx: u16) {
    r[0] = ax;
    r[1] = bx;
    r[3] = dx;
    r[9] = api;
    unsafe {
        dos_memory_call(r.as_mut_ptr());
    }
}
fn ems(r: &mut Regs, ax: u16, bx: u16, dx: u16) -> bool {
    call(r, EMS, ax, bx, dx);
    r[0] >> 8 == 0
}
fn xms(r: &mut Regs, ax: u16, dx: u16) -> bool {
    call(r, XMS, ax, 0, dx);
    r[0] == 1
}
fn dos(r: &mut Regs, ax: u16, bx: u16, es: u16) -> bool {
    r[6] = es;
    call(r, DOS, ax, bx, 0);
    r[10] & 1 == 0
}
fn transfer(r: &mut Regs, h: u16, reading: bool, buf: &mut [u16; 2]) -> bool {
    let seg = unsafe { dos_data_segment() };
    let off = buf.as_mut_ptr() as usize as u16;
    let mut desc = if reading {
        [4, 0, h, 0, 0, 0, off, seg]
    } else {
        [4, 0, 0, off, seg, h, 0, 0]
    };
    r[4] = desc.as_mut_ptr() as usize as u16;
    xms(r, 0x0b00, 0)
}
fn simultaneous(r: &mut Regs, x: u16, e: u16, u: u16, frame: u16) -> bool {
    let mut buf = [0x12abu16, 0x9432];
    if !transfer(r, x, false, &mut buf) || !ems(r, 0x4400, 0, e) {
        return false;
    }
    store(frame, 8, 0x52);
    store(u, 8, 0xa9);
    if !ems(r, 0x4400, 1, e) {
        return false;
    }
    store(frame, 8, 0xe7);
    if !ems(r, 0x4401, 0, e) || byte(frame + 0x400, 8) != 0x52 || byte(frame, 8) != 0xe7 {
        return false;
    }
    buf = [0, 0];
    transfer(r, x, true, &mut buf) && buf[0] == 0x12ab && buf[1] == 0x9432 && byte(u, 8) == 0xa9
}
fn in_chain(seg: u16, owner: u16) -> bool {
    let mut at = dosrs_realmode::PROGRAM - 1;
    while at < seg {
        let tag = byte(at, 0);
        if at + 1 == seg {
            return (tag == 77 || tag == 90) && w(at, 1) == owner;
        }
        if tag != 77 {
            return false;
        }
        let next = at.wrapping_add(1).wrapping_add(w(at, 3));
        if next <= at {
            return false;
        }
        at = next;
    }
    false
}
fn dos_upper(r: &mut Regs, other: u16) -> bool {
    if !dos(r, 0x6200, 0, 0) {
        return false;
    }
    let psp = r[1];
    if !dos(r, 0x5800, 0, 0) {
        return false;
    }
    let strategy = r[0];
    if !dos(r, 0x5802, 0, 0) {
        return false;
    }
    let link = r[0] & 255;
    let mut ok = false;
    if dos(r, 0x5803, 1, 0) && dos(r, 0x5801, 0x40, 0) && dos(r, 0x4800, 16, 0) {
        let seg = r[0];
        ok = seg >= 0xa000 && seg != other && in_chain(seg, psp) && byte(seg, 7) == 0;
        store(seg, 7, 0x5a);
        ok = dos(r, 0x4a00, 24, seg) && byte(seg, 7) == 0x5a && ok;
        ok = dos(r, 0x4900, 0, seg) && ok;
    }
    let a = dos(r, 0x5801, strategy, 0);
    let b = dos(r, 0x5803, link, 0);
    ok && a && b
}
fn test(r: &mut Regs, frame: u16, free: u16) -> bool {
    if !xms(r, 0x0900, 64) {
        return false;
    }
    let x = r[3];
    if !ems(r, 0x4300, 2, 0) {
        xms(r, 0x0a00, x);
        return false;
    }
    let e = r[3];
    if !xms(r, 0x1000, 32) {
        ems(r, 0x4500, 0, e);
        xms(r, 0x0a00, x);
        return false;
    }
    let u = r[1];
    call(r, VCPI, 0xde04, 0, 0);
    let mut ok = r[0] >> 8 == 0;
    if ok {
        let hi = r[11];
        let lo = r[3];
        ok = xms(r, 0x0c00, x);
        if ok {
            let xhi = r[3];
            let xlo = r[1];
            r[11] = xhi;
            call(r, VCPI, 0xde05, 0, xlo);
            ok = r[0] >> 8 != 0;
            ok = xms(r, 0x0d00, x) && ok;
        }
        ok = simultaneous(r, x, e, u, frame) && ok;
        if ok {
            text("XMS + EMS + UMB data verified\r\n");
        }
        ok = dos_upper(r, u) && ok;
        if ok {
            text("DOS high allocation, resize and free verified\r\n");
        }
        r[11] = hi;
        call(r, VCPI, 0xde05, 0, lo);
        ok = r[0] >> 8 == 0 && ok;
        r[11] = hi;
        call(r, VCPI, 0xde05, 0, lo);
        ok = r[0] >> 8 != 0 && ok;
    }
    ok = xms(r, 0x1100, u) && ok;
    ok = ems(r, 0x4500, 0, e) && ok;
    ok = xms(r, 0x0a00, x) && ok;
    if ems(r, 0x4300, 1, 0) {
        let h = r[3];
        ok = ems(r, 0x4400, 0, h) && byte(frame, 8) == 0 && ok;
        ok = ems(r, 0x4500, 0, h) && ok;
    } else {
        ok = false;
    }
    call(r, VCPI, 0xde03, 0, 0);
    ok && r[0] >> 8 == 0 && r[3] == free
}
#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    if argument(1, "/HIGH") {
        return high_memory::test();
    }
    if w(0, 0x67 * 4 + 2) == 0 || w(0, 0x2f * 4 + 2) == 0 {
        text("No 386 memory manager installed\r\n");
        return 1;
    }
    let mut r = [0; 12];
    call(&mut r, MULTIPLEX, 0x4300, 0, 0);
    if r[0] & 255 != 0x80 {
        return 1;
    }
    call(&mut r, MULTIPLEX, 0x4310, 0, 0);
    if argument(1, "/TEST")
        && byte(r[6], r[1]) != 0xea
        && (w(r[6], r[1]) != 0x03eb || w(r[6], r[1] + 2) != 0x9090 || byte(r[6], r[1] + 4) != 0x90)
    {
        text("Invalid XMS control header\r\n");
        return 1;
    }
    r[7] = r[1];
    r[8] = r[6];
    if argument(1, "/GEMMIS") {
        return import::test(&mut r);
    }
    call(&mut r, XMS, 0, 0, 0);
    let xv = r[0];
    if !ems(&mut r, 0x4600, 0, 0) {
        return 1;
    }
    let ev = r[0] & 255;
    text("EMS ");
    number((ev >> 4) as u32);
    text(".");
    number((ev & 15) as u32);
    text("  XMS ");
    number((xv >> 8) as u32);
    text(".");
    number((xv & 255) as u32);
    if !ems(&mut r, 0xde00, 0, 0) {
        return 1;
    }
    text("  VCPI ");
    number((r[1] >> 8) as u32);
    text(".");
    number((r[1] & 255) as u32);
    call(&mut r, VCPI, 0xde03, 0, 0);
    let free = r[3];
    text("  free KiB ");
    number(u32::from(free) * 4);
    text("\r\n");
    if !argument(1, "/TEST") {
        return 0;
    }
    if !handle_table::test(&mut r) {
        text("XMS handle table test FAILED\r\n");
        return 1;
    }
    text("XMS handle table allocation, lock, resize and release verified\r\n");
    if !ems(&mut r, 0x4100, 0, 0) {
        return 1;
    }
    let frame = r[1];
    if test(&mut r, frame, free) {
        text("PASS: page reuse, VCPI ownership and pool balance\r\n");
        0
    } else {
        text("Memory self-test FAILED\r\n");
        1
    }
}
