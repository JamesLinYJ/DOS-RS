//! Boot entry. One `kmain` per target binary.
//!
//! Serial I/O, the IDT, and the PIC mask stay in this file. The trap layout stays in `arch`.

#![no_std]
#![no_main]

use core::arch::asm;

mod arch;
mod bios;
mod cmd;
mod dos;
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
    let mut floppy = [0u8; 512];
    if bios::msdisk::read_floppy_boot(&mut floppy)
        && bios::msdisk::boot_sector_valid(&floppy)
        && bios::msdisk::bpb_bytes_per_sector(&floppy) == Some(512)
        && bios::msdisk::image_fat_media() == Some(0xF8)
        && bios::msdisk::fixed_disk_paths()
    {
        puts("DSKF55AA\n");
    }
    puts("MED");
    put_hex16(bios::msdisk::media_check() as u16);
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
    // Same 1:00:00.36 record, through TIME_TO_TICKS. The inverse is
    // CX=1 DX=0x000D.
    let written = [0x00, 0x00, 0x00, 0x01, 0x24, 0x00];
    let (hi, lo) = bios::msclock::write_clock(&written);
    puts("CLKW");
    put_hex16(hi);
    put_hex16(lo);
    puts("\n");
    let _prn = bios::mslpt::prn_write_bytes(b"PRN\r\n");
    if bios::mslpt::prn_stat() == 0x0100 {
        puts("PRNS\n");
    }
    puts("PRNR");
    put_hex16(bios::mslpt::prn_ioctl_get());
    puts("\n");
    // `PRN$TILBUSY` is a printer entry, not a SYSINIT step. The byte it
    // sends is the `.` on its own line. The count follows with the other
    // printer lines, before the loader and the missing interpreter.
    let sent = bios::mslpt::prn_tilbusy(b".");
    puts("\nPRNT");
    put_hex16(sent);
    puts("\n");
    if let Some(bytes) = bios::msload::load_io_sys() {
        puts("LOADOK\n");
        puts("LOAD");
        put_hex16(bytes);
        puts("\n");
    }
    let _messages = bios::sysimes::catalog_bytes();
    if let Ok(number) = bios::sysinit2::get_num(b"48") {
        puts("SYN");
        put_hex16(number);
        puts("\n");
    }
    puts("PAR");
    put_hex16(bios::sysinit1::para_round(17));
    puts("\n");
    puts("BUF");
    put_hex16(bios::sysinit1::default_buffers(false, false, false));
    puts("\n");
    if bios::sysinit1::config_line(b"BUFFERS=2\r") == Ok(Some(2))
        && bios::sysinit1::config_line(b"FOO\r").is_err()
    {
        bios::mscon::con_write_bytes(bios::sysimes::BADOPM);
        bios::mscon::con_write_bytes(b"\r\n");
    }
    if bios::sysinit1::round_mem(17, 0, 0x100).is_ok()
        && bios::sysinit2::skip_comment(b"; note\nBUFFERS=4\n") == 7
        && bios::sysinit2::parse_switches(b"\r", b"ABCD").is_err()
        && bios::sysinit2::parse_switches(b"/D\r", b"ABCD") == Ok(1 << 3)
        && bios::sysinit2::asciiz_len(b"COMMAND.COM\0") == 12
    {
        // The round and the comment skip agreed. The serial lines stay.
    }
    if crate::dos::exec::invoke() {
        puts("CMDOK\n");
        if let Some(payload) = crate::dos::exec::command_payload() {
            let _size = cmd::command::envdata::environ_len();
            if cmd::command::envdata::payload_is_comspec(payload) {
                puts("COMSPEC=");
                for &byte in cmd::command::envdata::ECOMSPEC {
                    putc(byte);
                }
                puts("\n");
            }
        }
        cmd::command::tcmd2a::print_default_prompt();
        puts("\n");
        let _ran = cmd::command::tmisc1::dispatch(b"VER");
        // The prompt loop runs the next typed line. Bare ECHO prints message
        // 1057. A tail that is not ON or OFF is printed from 82h. OFF clears
        // the bit, so the following bare ECHO prints the off text.
        let _echo = cmd::command::tmisc1::dispatch(b"ECHO\r");
        let _echo_text = cmd::command::tmisc1::dispatch(b"ECHO hello\r");
        let _echo_off = cmd::command::tmisc1::dispatch(b"ECHO OFF\r");
        let _echo_again = cmd::command::tmisc1::dispatch(b"ECHO\r");
        // `BREAK` with an empty tail reads CntCFlag. It starts clear, so the
        // line is message 1055 with "off". ON stores DL=1, and the next read
        // prints "on".
        let _break = cmd::command::tmisc1::dispatch(b"BREAK\r");
        let _break_on = cmd::command::tmisc1::dispatch(b"BREAK ON\r");
        let _break_again = cmd::command::tmisc1::dispatch(b"BREAK\r");
        // `VERIFY` reads VERFLG. It starts clear. ON stores AL bit 0 through
        // AH=2Eh, and the next AH=54h read prints message 1056 as on.
        let _verify = cmd::command::tmisc1::dispatch(b"VERIFY\r");
        let _verify_on = cmd::command::tmisc1::dispatch(b"VERIFY ON\r");
        let _verify_again = cmd::command::tmisc1::dispatch(b"VERIFY\r");
        // Bare PATH. The environment value after PATH= is a NUL, so
        // print_path takes the empty path and prints message 1049.
        // The next line stores `\DOS`. A second bare PATH prints it.
        let _path = cmd::command::tmisc1::dispatch(b"PATH\r");
        let _path_set = cmd::command::tmisc1::dispatch(b"PATH \\DOS\r");
        let _path_show = cmd::command::tmisc1::dispatch(b"PATH\r");
        // CLS asks INT 2Fh and clears through INT 10h.
        let _cls = cmd::command::tmisc1::dispatch(b"CLS\r");
        // AH=43h. Archive comes from the directory attribute. +R sets bit 0.
        let _attr = cmd::command::tmisc1::dispatch(b"ATTRIB COMMAND.COM\r");
        let _attr_set = cmd::command::tmisc1::dispatch(b"ATTRIB +R COMMAND.COM\r");
        // AH=57h stores 1984-03-04 01:02, then DIR prints that stamp.
        cmd::command::tcmd1a::stamp_command();
        // SET with no tail lists the block. FOO=BAR is stored, then listed.
        let _set = cmd::command::tmisc1::dispatch(b"SET FOO=BAR\r");
        let _set_show = cmd::command::tmisc1::dispatch(b"SET\r");
        // DATE and TIME read AH=2Ah and AH=2Ch, then store a new value.
        let _date = cmd::command::tmisc1::dispatch(b"DATE\r");
        let _date_set = cmd::command::tmisc1::dispatch(b"DATE 3-4-1984\r");
        let _date_show = cmd::command::tmisc1::dispatch(b"DATE\r");
        let _time = cmd::command::tmisc1::dispatch(b"TIME\r");
        let _time_set = cmd::command::tmisc1::dispatch(b"TIME 1:02:03.04\r");
        let _time_show = cmd::command::tmisc1::dispatch(b"TIME\r");
        // VOL searches for a volume label. This disk has none.
        let _vol = cmd::command::tmisc1::dispatch(b"VOL\r");
        // DIR lists the boot root. The names are the 8.3 fields.
        let _dir = cmd::command::tmisc1::dispatch(b"DIR\r");
        // TYPE prints the file bytes. NONE is the missing-file line.
        let _typed = cmd::command::tmisc1::dispatch(b"TYPE COMMAND.COM\r");
        let _type_miss = cmd::command::tmisc1::dispatch(b"TYPE NONE\r");
        // DEL removes IO.SYS. The following DIR no longer lists it.
        let _del = cmd::command::tmisc1::dispatch(b"DEL IO.SYS\r");
        let _erased = cmd::command::tmisc1::dispatch(b"DIR\r");
        // REN changes COMMAND.COM to FOO.COM. The next DIR shows the new name.
        let _ren = cmd::command::tmisc1::dispatch(b"REN COMMAND.COM FOO.COM\r");
        let _renamed = cmd::command::tmisc1::dispatch(b"DIR\r");
        // COPY adds BAR.COM. The next DIR lists it.
        let _copy = cmd::command::tmisc1::dispatch(b"COPY FOO.COM BAR.COM\r");
        let _copied = cmd::command::tmisc1::dispatch(b"DIR\r");
        // The copy wrote the source bytes. TYPE reads them back with AH=3Fh.
        let _copy_type = cmd::command::tmisc1::dispatch(b"TYPE BAR.COM\r");
        // MD makes SUB, CD prints it as the directory header, RD removes it.
        let _md = cmd::command::tmisc1::dispatch(b"MD SUB\r");
        let _md_dir = cmd::command::tmisc1::dispatch(b"DIR\r");
        let _cd = cmd::command::tmisc1::dispatch(b"CD SUB\r");
        let _cd_dir = cmd::command::tmisc1::dispatch(b"DIR\r");
        let _cd_up = cmd::command::tmisc1::dispatch(b"CD \\\r");
        let _rd = cmd::command::tmisc1::dispatch(b"RD SUB\r");
        let _rd_dir = cmd::command::tmisc1::dispatch(b"DIR\r");
        // PROMPT $P$G expands the current directory and the greater-than sign.
        let _prompt = cmd::command::tmisc1::dispatch(b"PROMPT $P$G\r");
        let _if = cmd::command::tmisc1::dispatch(b"IF EXIST FOO.COM ECHO ifok\r");
        let _call = cmd::command::tmisc1::dispatch(b"CALL\r");
        let _goto = cmd::command::tmisc1::dispatch(b"GOTO NOWHERE\r");
        let _for = cmd::command::tmisc1::dispatch(b"FOR %A IN (X) DO ECHO forok\r");
        let _shift = cmd::command::tmisc1::dispatch(b"SHIFT\r");
        // FCB open of FOO.COM after the rename. The line is the file length
        // and the sequential-read return.
        dos::fcb::show();
        // Extend cluster 3 on the FAT12 image, then the same step on a FAT16 table.
        dos::rom::show();
        // Seek within BAR.COM, write the first byte back, and close the handle.
        dos::close::show();
        // Media check from the rest of FAT.ASM: unchanged, changed, dirty, error.
        dos::fat::media_show();
        // Drive parameter block, buffer cache, and the sector read under it.
        dos::buf::show();
        // IOCTL on a closed handle, removable media, and the path checks.
        dos::ioctl::show();
        // Exec rejects AL=2, misses a name, and loads COMMAND.COM as a COM file.
        dos::exec::show();
    } else {
        // EXEC returned carry. `COMERR` is only that path.
        bios::sysinit1::com_err();
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
        (*idt)[0x2F] = IdtEntry::interrupt(cmd::command::tcmd2a::int2f_gate_addr(), selector);
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
            clobber_abi("C"),
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
            "mov eax, 0x3000",
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
            clobber_abi("C"),
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
