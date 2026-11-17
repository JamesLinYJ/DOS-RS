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
    fn dos_memory_call(regs: *mut u16);
    fn dos_drive() -> u16;
    fn dos_ui(op: u16, a: u16, b: u16) -> u16;
    fn arg(index: u16, at: u16) -> u16;
    fn disk_size(unit: u16) -> u32;
}
/// Extend this process's block into adjacent free paragraphs for file or media buffers.
fn workspace_end() -> u16 {
    let mut regs = [0u16; 12];
    regs[0] = 0x6200;
    regs[9] = 3;
    // The architecture library executes INT 21h with this register frame.
    unsafe { dos_memory_call(regs.as_mut_ptr()) };
    if regs[10] & 1 != 0 {
        return 0;
    }
    let psp = regs[1];
    let initial = dosrs_realmode::system::w(psp, 2);
    regs[0] = 0x4a00;
    regs[1] = u16::MAX;
    regs[6] = psp;
    unsafe { dos_memory_call(regs.as_mut_ptr()) };
    let available = regs[1];
    if regs[10] & 1 == 0 || regs[0] != 8 || available <= initial - psp {
        return initial;
    }
    regs[0] = 0x4a00;
    regs[6] = psp;
    unsafe { dos_memory_call(regs.as_mut_ptr()) };
    if regs[10] & 1 == 0 {
        psp + available
    } else {
        initial
    }
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
