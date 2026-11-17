pub(crate) fn kmain(handoff: boot::Handoff, firmware: &str) -> ! {
    heap::initialize();
    arch::install_interrupts();
    bios::pc::configure_serial(0x3f8, 0xe3);
    let attr = bios::pc::color(0x0b);
    puts(banner());
    bios::pc::color(attr);
    puts(firmware);
    puts("\n");
    puts("Physical frames available: ");
    platform::number(arch::frames::counts().0 as u32);
    puts("\n");
    platform::initialize(handoff);
    #[cfg(feature = "tlb-selftest")]
    if let Err(error) = arch::user::tlb_test::run() {
        puts(error);
        debug_exit(false);
    }
    if cfg!(feature = "native-gate-test") {
        gate_test_wait();
    }
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
        tasks::interactive()
    };
    debug_exit(success)
}

// The external CPU-gate regression owns this buffer while the BSP is idle.
fn gate_test_wait() -> ! {
    static mut BUFFER: [u8; 512] = [0; 512];
    vfs::dos::boot();
    puts("Native gate test buffer ");
    for byte in (&raw mut BUFFER as usize).to_be_bytes() {
        put_hex8(byte)
    }
    puts("\n");
    loop {
        core::hint::spin_loop()
    }
}

fn banner() -> &'static str {
    #[cfg(target_arch = "x86")]
    {
        "DOS / i686\n"
    }
    #[cfg(target_arch = "x86_64")]
    {
        "DOS / x86_64\n"
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
fn panic(info: &core::panic::PanicInfo) -> ! {
    if let Some(loc) = info.location() {
        for b in loc.file().bytes() {
            arch::outb(0x3f8, b);
        }
        for b in loc.line().to_be_bytes() {
            for part in [b >> 4, b & 15] {
                arch::outb(0x3f8, b"0123456789ABCDEF"[usize::from(part)]);
            }
        }
        arch::outb(0x3f8, 10);
    }
    loop {}
}
