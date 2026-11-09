//! Boot entry. One `kmain` per target binary.
//!
//! Serial I/O, the IDT, and the PIC mask stay in this file. The trap layout stays in `arch`.

#![no_std]
#![no_main]

use core::arch::asm;

mod arch;
mod bios;
mod int21;

/// Multiboot calls this. It does not return.
#[no_mangle]
pub extern "C" fn kmain() -> ! {
    boot(arch::gate_addr())
}

fn boot(gate: usize) -> ! {
    mask_pic();
    load_idt(gate);
    puts(banner());
    let line = invoke_int21();
    puts_result(&line);
    bios::mscon::con_write_bytes(b"BIOS\r\n");
    bios::msaux::aux_write_bytes(b"AUX\r\n");
    let mut sector = [0u8; 512];
    let disk_status = bios::msdisk::read_sector0(&mut sector);
    puts("DSK");
    put_hex16(disk_status);
    puts("\n");
    let mut clock = [0u8; 6];
    let _clock_status = bios::msclock::read_clock(&mut clock);
    puts("CLK");
    put_hex16(u16::from_le_bytes([clock[0], clock[1]]));
    put_hex16(u16::from_le_bytes([clock[2], clock[3]]));
    puts("\n");
    // INT 1Ah is entered with CX=1 and DX=0x0E. The gate must hand those
    // ticks back, and TIM$READ then stores 1:00:00.36.
    let mut forced = [0u8; 6];
    let _forced = bios::msclock::read_clock_at(&mut forced, 1, 0x0E);
    puts("CLKP");
    for byte in forced {
        put_hex8(byte);
    }
    puts("\n");
    let _prn = bios::mslpt::prn_write_bytes(b"PRN\r\n");
    if bios::msload::load_io_sys() {
        puts("LOADOK\n");
    }
    debug_exit()
}

fn banner() -> &'static str {
    #[cfg(target_arch = "x86")]
    {
        "dos4-rs i686\n"
    }
    #[cfg(target_arch = "x86_64")]
    {
        "dos4-rs x86_64\n"
    }
}

/// Low 16 bits and carry after `int 0x21` returns to the caller.
struct VersionLine {
    ax: u16,
    bx: u16,
    cx: u16,
    carry: bool,
}

/// Writes `0xFF` to both PIC mask ports so IRQ1 cannot raise vector `0x21`.
///
/// Port `0x21` is the master PIC mask. Vector `0x21` is the IDT slot.
/// Writing `0xFF` again leaves the masks the same.
fn mask_pic() {
    outb(0x21, 0xFF);
    outb(0xA1, 0xFF);
}

const GATE_INTERRUPT: u8 = 0x8E;

#[cfg(target_arch = "x86")]
#[derive(Clone, Copy)]
#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    zero: u8,
    type_attr: u8,
    offset_high: u16,
}

#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy)]
#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const fn empty() -> Self {
        #[cfg(target_arch = "x86")]
        {
            Self {
                offset_low: 0,
                selector: 0,
                zero: 0,
                type_attr: 0,
                offset_high: 0,
            }
        }
        #[cfg(target_arch = "x86_64")]
        {
            Self {
                offset_low: 0,
                selector: 0,
                ist: 0,
                type_attr: 0,
                offset_mid: 0,
                offset_high: 0,
                reserved: 0,
            }
        }
    }

    fn interrupt(gate: usize, selector: u16) -> Self {
        #[cfg(target_arch = "x86")]
        {
            Self {
                offset_low: gate as u16,
                selector,
                zero: 0,
                type_attr: GATE_INTERRUPT,
                offset_high: (gate >> 16) as u16,
            }
        }
        #[cfg(target_arch = "x86_64")]
        {
            Self {
                offset_low: gate as u16,
                selector,
                ist: 0,
                type_attr: GATE_INTERRUPT,
                offset_mid: (gate >> 16) as u16,
                offset_high: (gate >> 32) as u32,
                reserved: 0,
            }
        }
    }
}

#[repr(C, packed)]
struct IdtPtr {
    limit: u16,
    #[cfg(target_arch = "x86")]
    base: u32,
    #[cfg(target_arch = "x86_64")]
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::empty(); 256];

/// Loads interrupt gates for INT 10h, INT 16h, INT 21h, and INT 29h.
///
/// An interrupt gate clears IF on entry.
fn load_idt(gate: usize) {
    let selector: u16;
    unsafe {
        asm!("mov {sel:x}, cs", sel = out(reg) selector, options(nomem, nostack, preserves_flags));
    }
    let entry = IdtEntry::interrupt(gate, selector);
    unsafe {
        let idt = &raw mut IDT;
        (*idt)[0x10] = IdtEntry::interrupt(bios::mscon::int10_gate_addr(), selector);
        (*idt)[0x13] = IdtEntry::interrupt(bios::msdisk::int13_gate_addr(), selector);
        (*idt)[0x1A] = IdtEntry::interrupt(bios::msclock::int1a_gate_addr(), selector);
        (*idt)[0x14] = IdtEntry::interrupt(bios::msaux::int14_gate_addr(), selector);
        (*idt)[0x17] = IdtEntry::interrupt(bios::mslpt::int17_gate_addr(), selector);
        (*idt)[0x16] = IdtEntry::interrupt(bios::mscon::int16_gate_addr(), selector);
        (*idt)[0x21] = entry;
        (*idt)[0x29] = IdtEntry::interrupt(bios::mscon::int29_gate_addr(), selector);
        let ptr = IdtPtr {
            limit: (core::mem::size_of_val(&*idt) - 1) as u16,
            #[cfg(target_arch = "x86")]
            base: idt as u32,
            #[cfg(target_arch = "x86_64")]
            base: idt as u64,
        };
        asm!("lidt [{ptr}]", ptr = in(reg) &ptr, options(readonly, nostack, preserves_flags));
    }
}

/// Sets carry, sets AH to `0x30`, and executes `int 0x21`.
///
/// This is the only invocation. It does not call `int21::entry`.
/// Carry is set first so the serial line can show that AH=30h left it set.
fn invoke_int21() -> VersionLine {
    #[cfg(target_arch = "x86")]
    let (ax, bx, cx, flags) = unsafe {
        let ax: u32;
        let bx: u32;
        let cx: u32;
        let flags: u32;
        asm!(
            "stc",
            "mov ah, 0x30",
            "int 0x21",
            "pushfd",
            "pop edx",
            lateout("eax") ax,
            lateout("ebx") bx,
            lateout("ecx") cx,
            lateout("edx") flags,
        );
        (ax, bx, cx, flags)
    };
    #[cfg(target_arch = "x86_64")]
    let (ax, bx, cx, flags) = unsafe {
        let ax: u64;
        let bx: u64;
        let cx: u64;
        let flags: u64;
        asm!(
            "stc",
            "mov ah, 0x30",
            "int 0x21",
            "mov r8, rax",
            "mov r9, rcx",
            "pushfq",
            "pop r10",
            "push rbx",
            "pop rax",
            lateout("r8") ax,
            lateout("rax") bx,
            lateout("r9") cx,
            lateout("r10") flags,
        );
        (ax, bx, cx, flags)
    };
    VersionLine {
        ax: ax as u16,
        bx: bx as u16,
        cx: cx as u16,
        carry: flags & 1 == 1,
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
    put_hex16(line.ax);
    puts(" BX=");
    put_hex16(line.bx);
    puts(" CX=");
    put_hex16(line.cx);
    puts(" CF=");
    putc(if line.carry { b'1' } else { b'0' });
    puts("\n");
}

/// Writes `0x10` to port `0xF4`.
///
/// QEMU `isa-debug-exit` returns status `(0x10 << 1) | 1`, which is 33.
/// The smoke script maps 33 to process status 0.
fn debug_exit() -> ! {
    outb(0xF4, 0x10);
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

fn outb(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
