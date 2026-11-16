#![no_std]
pub mod editor;
pub mod input;
pub mod install;
pub mod media_set;
pub mod partition;
pub mod startup;
pub mod ui;
unsafe extern "Rust" {
    fn putch(c: u16) -> u16;
    fn dos_key() -> u16;
    fn dos_drive() -> u16;
    fn dos_ui(op: u16, a: u16, b: u16) -> u16;
    fn dos_line(p: *const u8, n: u16);
    fn arg(index: u16, at: u16) -> u16;
    fn disk_size(unit: u16) -> u32;
}
pub fn put(c: u16) {
    unsafe {
        putch(c);
    }
}
pub fn text(s: &str) {
    let mut i = 0;
    while i < s.len() {
        put(s.as_bytes()[i] as u16);
        i += 1;
    }
}
pub fn key() -> u16 {
    unsafe { dos_key() }
}
pub fn drive() -> u16 {
    unsafe { dos_drive() }
}
pub fn submit(s: &[u8], n: u16) {
    unsafe { dos_line(s.as_ptr(), n) }
}
pub fn argument(i: u16, s: &str) -> bool {
    let mut j = 0;
    while j < s.len() {
        let mut c = unsafe { arg(i, j as u16) };
        if c >= 97 && c <= 122 {
            c -= 32
        }
        if c != s.as_bytes()[j] as u16 {
            return false;
        }
        j += 1;
    }
    unsafe { arg(i, j as u16) == 0 }
}
pub fn capacity() -> u32 {
    unsafe { disk_size(dosrs_realmode::hard_unit()) }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

pub fn screen(op: u16, a: u16, b: u16) -> u16 {
    unsafe { dos_ui(op, a, b) }
}
