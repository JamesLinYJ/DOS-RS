//! A 16-bit user of the public multiplex ABI. All wide addresses travel in
//! three words; the page itself is accessed through a checked DOS buffer.
use super::{MULTIPLEX, Regs, call, dos_data_segment, number, text, w};
use dos_tools::put;

fn request(r: &mut Regs, function: u16, bx: u16, cx: u16, dx: u16) -> u16 {
    r[5] = 0x4850;
    r[2] = cx;
    call(r, MULTIPLEX, function, bx, dx);
    if r[5] == 0x5048 { r[0] } else { 0 }
}
fn word_hex(value: u16) {
    let mut bits = 16;
    while bits > 0 {
        bits -= 4;
        put(b"0123456789ABCDEF"[((value >> bits) & 15) as usize] as u16);
    }
}
fn failed() -> u16 {
    text("HIGH PAGE FAILED\r\n");
    1
}
pub(super) fn test() -> u16 {
    let mut r = [0; 12];
    if w(0, 0x2f * 4 + 2) == 0 || request(&mut r, 0xd7b0, 0, 0, 0) == 0 {
        text("No high physical RAM available\r\n");
        return 0;
    }
    text("Physical address bits ");
    number(r[1] as u32);
    text("\r\n");
    let first = request(&mut r, 0xd7b1, 0, 0, 1);
    if first == 0 || r[3] == 0 {
        return failed();
    }
    let second = request(&mut r, 0xd7b1, 0, 0, 1);
    if second == 0 || second == first || r[3] == 0 {
        return failed();
    }
    text("Second physical page 0x");
    word_hex(r[3]);
    word_hex(r[2]);
    word_hex(r[1]);
    text("\r\n");
    let mut buf = [0x48u8, 0x69, 0xa5, 0x3c];
    r[4] = buf.as_mut_ptr() as usize as u16;
    r[6] = unsafe { dos_data_segment() };
    if request(&mut r, 0xd7b3, 4092, 4, second) != 1 {
        return failed();
    }
    if request(&mut r, 0xd7b3, 4093, 4, second) != 0 {
        return failed();
    }
    if request(&mut r, 0xd7b2, 4092, 4, first) != 1 || buf != [0; 4] {
        return failed();
    }
    if request(&mut r, 0xd7b2, 4092, 4, second) != 1 || buf != [0x48, 0x69, 0xa5, 0x3c] {
        return failed();
    }
    if request(&mut r, 0xd7b3, 0, 4, first) != 1 || request(&mut r, 0xd7b4, 0, 0, first) != 1 {
        return failed();
    }
    let next = request(&mut r, 0xd7b1, 0, 0, 1);
    if next == 0
        || next == first
        || request(&mut r, 0xd7b2, 0, 1, first) != 0
        || request(&mut r, 0xd7b2, 0, 4, next) != 1
        || buf != [0; 4]
    {
        return failed();
    }
    if request(&mut r, 0xd7b4, 0, 0, next) != 1
        || request(&mut r, 0xd7b4, 0, 0, second) != 1
        || request(&mut r, 0xd7b1, 0xffff, 0xffff, 0xffff) != 0
    {
        return failed();
    }
    text("PASS: 16-bit high page transfer, isolation, bounds and stale handles\r\n");
    0
}
