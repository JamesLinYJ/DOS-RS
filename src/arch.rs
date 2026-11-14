//! Privileged operations used only by the kernel's device owners.
pub(crate) mod io_memory;
pub(crate) mod platform;
mod traps;
pub(crate) fn outw(port: u16, v: u16) {
    unsafe {
        x86::io::outw(port, v);
    }
}
pub(crate) use traps::{install_interrupts, load_interrupts};

// The UEFI entry and secondary threads run at CPL 0. Guest port numbers are
// filtered in bios::pc before reaching these physical port accessors.
pub(crate) fn outb(port: u16, value: u8) {
    unsafe { x86::io::outb(port, value) }
}
pub(crate) fn inb(port: u16) -> u8 {
    unsafe { x86::io::inb(port) }
}
pub(crate) fn inw(port: u16) -> u16 {
    unsafe { x86::io::inw(port) }
}
pub(crate) fn halt() -> ! {
    loop {
        unsafe { x86::halt() }
    }
}

pub(crate) struct GateToken(());
impl GateToken {
    pub(crate) fn seal(&self) {}
}
