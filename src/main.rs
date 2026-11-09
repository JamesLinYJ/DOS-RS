//! Boot entry. One `kmain` per target binary.
//!
//! Serial I/O, the IDT, and the PIC mask stay in this file. The trap layout stays in `arch`.

#![no_std]
#![no_main]

mod arch;
mod int21;

/// Multiboot calls this. It does not return.
///
/// The body masks the PIC, installs the INT 21h gate, executes `int 0x21`,
/// and leaves through `isa-debug-exit`.
#[no_mangle]
pub extern "C" fn kmain() -> ! {
    boot(arch::gate_addr())
}

fn boot(gate: usize) -> ! {
    mask_pic();
    load_idt(gate);
    puts("dos4-rs\n");
    invoke_int21();
    puts_result();
    debug_exit()
}

/// Writes `0xFF` to both PIC mask ports so IRQ1 cannot raise vector `0x21`.
///
/// Port `0x21` is the master PIC mask. Vector `0x21` is the IDT slot.
/// Writing `0xFF` again leaves the masks the same.
fn mask_pic() {
    // TODO: outb(0x21, 0xFF) and outb(0xA1, 0xFF) before lidt.
    unimplemented!("mask both PICs")
}

/// Loads an IDT whose vector `0x21` is an interrupt gate at `gate`.
///
/// An interrupt gate clears IF on entry. No other vector is installed.
fn load_idt(gate: usize) {
    let _ = gate;
    // TODO: build 256 gates, store `gate` at index 0x21, then lidt.
    unimplemented!("load the IDT")
}

/// Sets carry, sets AH to `0x30`, and executes `int 0x21`.
///
/// This is the only invocation. It does not call `int21::entry`.
/// Carry is set first so the serial line can show that AH=30h left it set.
fn invoke_int21() {
    // TODO: stc; mov ah, 0x30; int 0x21. Leave AL as it is.
    unimplemented!("int 0x21")
}

fn puts(text: &str) {
    let _ = text;
    // TODO: poll COM1 LSR at port 0x3FD bit 5, then write the byte to port 0x3F8.
    unimplemented!("COM1")
}

/// Prints the low 16 bits of AX, BX, and CX, and the carry bit, after `int 0x21`.
///
/// Those bits are in the CPU registers. This function does not take a logical frame.
fn puts_result() {
    // TODO: print AX, BX, and CX as four hex digits, and CF as 0 or 1.
    unimplemented!("print AH=30h")
}

/// Writes `0x10` to port `0xF4`.
///
/// QEMU `isa-debug-exit` returns status `(0x10 << 1) | 1`, which is 33.
/// The smoke script maps 33 to process status 0.
fn debug_exit() -> ! {
    // TODO: outb(0xF4, 0x10), then hlt.
    unimplemented!("isa-debug-exit")
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
