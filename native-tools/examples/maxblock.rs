#![no_std]
#![no_main]
use dos_tools::{install::number, text};
use dosrs_realmode::{byte, store, system::w};

unsafe extern "Rust" {
    fn dos_data_segment() -> u16;
    fn dos_memory_call(regs: *mut u16);
}

fn call(ax: u16, bx: u16, es: u16) -> (u16, u16, bool) {
    let mut r = [0u16; 12];
    r[0] = ax;
    r[1] = bx;
    r[6] = es;
    r[9] = 3;
    unsafe { dos_memory_call(r.as_mut_ptr()) };
    (r[0], r[1], r[10] & 1 == 0)
}

fn probe() -> Result<u16, u16> {
    let (error, largest, ok) = call(0x4800, u16::MAX, 0);
    if ok || error != 8 || largest < 10000 {
        return Err(2);
    }
    let (start, _, granted) = call(0x4800, largest, 0);
    if !granted {
        return Err(3);
    }
    let end = start + largest - 1;
    store(start, 0, 0x31);
    store(end, 15, 0x82);
    let matches = byte(start, 0) == 0x31 && byte(end, 15) == 0x82;
    if !call(0x4900, 0, start).2 || !matches {
        return Err(4);
    }
    Ok(largest)
}

#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    let psp = call(0x6200, 0, 0).1;
    let initial = w(psp, 2) - psp;
    let ds = unsafe { dos_data_segment() };
    if !call(0x4a00, ds - psp + 1024, psp).2 {
        return 1;
    }
    let (linked, _, has_umb) = call(0x5802, 0, 0);
    if has_umb && linked != 0 && !call(0x5803, 0, 0).2 {
        return 5;
    }
    let result = probe();
    if has_umb && linked != 0 && !call(0x5803, linked, 0).2 {
        return 5;
    }
    let largest = match result {
        Ok(paragraphs) => paragraphs,
        Err(code) => return code,
    };
    text("MAXBLOCK initial=");
    number(u32::from(initial) * 16);
    text(" allocated-and-readback=");
    number(u32::from(largest) * 16);
    text(" bytes. Exit=17\r\n");
    17
}
