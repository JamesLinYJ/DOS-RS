//! Boot entry. One `kmain` per target binary.
//!
//! Starts the kernel services, then enters the interactive DOS scheduler or the selected test profile.

#![no_std]
#![no_main]

mod arch;
mod bios;
mod boot;
mod cmd;
mod dos;
mod drivers;
mod guest;
mod int21;
mod kernel_cell;
mod platform;
mod smoke;

pub(crate) fn kmain(handoff: boot::Handoff, firmware: &str) -> ! {
    arch::install_interrupts();
    bios::pc::configure_serial(0x3f8, 0xe3);
    let attr = bios::pc::color(0x0b);
    puts(banner());
    bios::pc::color(attr);
    puts(firmware);
    puts("\n");
    platform::initialize(handoff);
    let success = if cfg!(feature = "kernel-selftest") {
        smoke::run();
        true
    } else if cfg!(feature = "batch-console") {
        let completed = guest::run::run_shell();
        if completed {
            puts("\nBatch complete.\n");
        }
        completed
    } else {
        guest::run::interactive()
    };
    debug_exit(success)
}

fn banner() -> &'static str {
    #[cfg(target_arch = "x86")]
    {
        "DOS-RS / i686\n"
    }
    #[cfg(target_arch = "x86_64")]
    {
        "DOS-RS / x86_64\n"
    }
}

/// Low 16 bits and carry after `int 0x21` returns to the caller.
struct VersionLine {
    version: u16,
    oem: u16,
    serial: u16,
    carry: bool,
}

fn invoke_int21(carry_in: bool) -> VersionLine {
    let (version, oem, serial, carry) = int21::version_call(carry_in);
    VersionLine {
        version,
        oem,
        serial,
        carry,
    }
}

fn puts(text: &str) {
    for byte in text.bytes() {
        if byte == b'\n' {
            putc(b'\r');
        }
        putc(byte);
    }
}

pub(crate) fn serial_putc(byte: u8) {
    putc(byte);
}

fn putc(byte: u8) {
    bios::pc::display(byte);
    while inb(0x3FD) & 0x20 == 0 {}
    outb(0x3F8, byte);
}

fn put_hex8(value: u8) {
    let digits = b"0123456789ABCDEF";
    putc(digits[(value >> 4) as usize]);
    putc(digits[(value & 0xF) as usize]);
}

fn put_hex16(mut value: u16) {
    let digits = b"0123456789ABCDEF";
    let mut out = [0u8; 4];
    let mut index = 4;
    while index > 0 {
        index -= 1;
        out[index] = digits[(value & 0xF) as usize];
        value >>= 4;
    }
    for byte in out {
        putc(byte);
    }
}

/// Prints the low 16 bits of AX, BX, and CX, and the carry bit.
fn puts_result(line: &VersionLine) {
    puts("AX=");
    put_hex16(line.version);
    puts(" BX=");
    put_hex16(line.oem);
    puts(" CX=");
    put_hex16(line.serial);
    puts(" CF=");
    putc(if line.carry { b'1' } else { b'0' });
    puts("\n");
}

/// Report the boot outcome to QEMU and halt.
///
/// QEMU `isa-debug-exit` returns status `(0x10 << 1) | 1`, which is 33.
/// The smoke script maps 33 to process status 0.
fn debug_exit(success: bool) -> ! {
    outb(0xF4, if success { 0x10 } else { 0x11 });
    arch::halt()
}

fn outb(port: u16, value: u8) {
    arch::outb(port, value);
}

fn inb(port: u16) -> u8 {
    arch::inb(port)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
