//! Multiboot trampoline and the trap-to-logical mapping.
//!
//! The trampoline establishes the native stack and calls `kmain`.
//! This module does not own the DOS register contract.

use core::arch::global_asm;

use crate::int21::{self, LogicalFrame, NativeRegs};

global_asm!(
    r#"
.section .multiboot, "a"
.balign 4
mb_header:
.long 0x1BADB002
.long 0x00010000
.long -(0x1BADB002 + 0x00010000)
.long mb_header
.long __load_start
.long __load_end
.long __bss_end
.long _start
"#
);

#[cfg(target_arch = "x86")]
global_asm!(
    r#"
.section .text.boot, "ax"
.code32
.global _start
_start:
    mov esp, offset __stack_top
    cld
    call kmain
2:  hlt
    jmp 2b
"#
);

#[cfg(target_arch = "x86_64")]
global_asm!(
    r#"
.section .bss.boot, "aw", @nobits
.balign 4096
pml4: .skip 4096
pdpt: .skip 4096
pd:   .skip 4096
.section .rodata
.balign 8
gdt64:
    .quad 0
    .quad 0x00209A0000000000
    .quad 0x0000920000000000
gdt64_ptr:
    .word gdt64_ptr - gdt64 - 1
    .long gdt64
.section .text.boot, "ax"
.code32
.global _start
_start:
    mov esp, offset __stack_top
    mov eax, offset pdpt
    or eax, 3
    mov dword ptr [pml4], eax
    mov eax, offset pd
    or eax, 3
    mov dword ptr [pdpt], eax
    xor ecx, ecx
3:  mov eax, ecx
    shl eax, 21
    or eax, 0x83
    mov dword ptr [pd + ecx*8], eax
    inc ecx
    cmp ecx, 512
    jne 3b
    mov eax, offset pml4
    mov cr3, eax
    mov eax, cr4
    or eax, 0x20
    mov cr4, eax
    mov ecx, 0xC0000080
    rdmsr
    or eax, 0x100
    wrmsr
    mov eax, cr0
    or eax, 0x80000000
    mov cr0, eax
    lgdt [gdt64_ptr]
    .byte 0xEA
    .long long_mode
    .word 0x08
.code64
long_mode:
    xor eax, eax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov rsp, offset __stack_top
    cld
    call kmain
4:  hlt
    jmp 4b
"#
);

/// Proof that the caller is the arch gate.
///
/// The constructor is private to this module. `kmain` cannot mint one, so it cannot call `int21::entry`.
pub(crate) struct GateToken {
    _priv: (),
}

impl GateToken {
    fn mint() -> Self {
        Self { _priv: () }
    }

    #[allow(clippy::let_unit_value)]
    pub(crate) fn seal(&self) {
        let _ = self._priv;
    }
}

/// Address used when installing the native INT 21h gate.
fn gate_addr() -> usize {
    int21_gate as *const () as usize
}

unsafe extern "C" {
    fn int21_gate();
}

/// Software-saved registers in stack order. The CPU return frame stays above
/// these slots and is consumed directly by IRET.
#[repr(C)]
struct TrapFrame {
    es: usize,
    ds: usize,
    di: usize,
    si: usize,
    bp: usize,
    #[cfg(target_arch = "x86")]
    _original_sp: usize, // PUSHAD saves ESP. POPAD skips this slot.
    bx: usize,
    dx: usize,
    cx: usize,
    ax: usize,
}

#[cfg(target_arch = "x86")]
const _: () = assert!(core::mem::size_of::<TrapFrame>() == 40);
#[cfg(target_arch = "x86_64")]
const _: () = assert!(core::mem::size_of::<TrapFrame>() == 72);

#[cfg(target_arch = "x86")]
global_asm!(
    r#"
.section .text, "ax"
.code32
.global int21_gate
int21_gate:
    pushad
    xor eax, eax
    mov ax, ds
    push eax
    mov ax, es
    push eax
    mov ebx, esp
    and esp, -16
    sub esp, 12
    push ebx
    cld
    call {dispatch}
    mov esp, ebx
    pop eax
    mov es, ax
    pop eax
    mov ds, ax
    popad
    iretd
"#,
    dispatch = sym dispatch_from_trap,
);

#[cfg(target_arch = "x86_64")]
global_asm!(
    r#"
.section .text, "ax"
.code64
.global int21_gate
int21_gate:
    sub rsp, 128
    push r8
    push r9
    push r10
    push r11
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    xor eax, eax
    mov ax, ds
    push rax
    mov ax, es
    push rax
    mov rbx, rsp
    mov rdi, rbx
    and rsp, -16
    cld
    call {dispatch}
    mov rsp, rbx
    pop rax
    mov es, ax
    pop rax
    mov ds, ax
    pop rdi
    pop rsi
    pop rbp
    pop rbx
    pop rdx
    pop rcx
    pop rax
    pop r11
    pop r10
    pop r9
    pop r8
    add rsp, 128
    iretq
"#,
    dispatch = sym dispatch_from_trap,
);

extern "C" fn dispatch_from_trap(trap: &mut TrapFrame) {
    let token = GateToken::mint();
    let regs = NativeRegs {
        status: trap.ax as u64,
        handle: trap.bx as u64,
        count: trap.cx as u64,
        buffer: trap.dx as u64,
        source: trap.si as u64,
        destination: trap.di as u64,
        base: trap.bp as u64,
        data_segment: trap.ds as u64,
        extra_segment: trap.es as u64,
    };
    let mut frame = LogicalFrame::from_native(&token, regs);
    int21::entry(&token, &mut frame);
    let regs = frame.native(&token);
    trap.ax = regs.status as usize;
    trap.bx = regs.handle as usize;
    trap.cx = regs.count as usize;
    trap.dx = regs.buffer as usize;
    trap.si = regs.source as usize;
    trap.di = regs.destination as usize;
    trap.bp = regs.base as usize;
    trap.ds = regs.data_segment as usize;
    trap.es = regs.extra_segment as usize;
}

/// Writes `0xFF` to both PIC mask ports so IRQ1 cannot raise vector `0x21`.
///
/// Port `0x21` is the master PIC mask. Vector `0x21` is the IDT slot.
/// Writing `0xFF` again leaves the masks the same.
pub(super) fn install_interrupts() {
    outb(0x21, 0xFF);
    outb(0xA1, 0xFF);
    install_dos_gate();
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

/// Installs the native DOS gate in IDT slot 21h.
///
/// An interrupt gate clears IF on entry.
fn install_dos_gate() {
    let selector = code_selector();
    let entry = IdtEntry::interrupt(gate_addr(), selector);
    unsafe {
        let idt = &raw mut IDT;
        (*idt)[0x21] = entry;
        let ptr = IdtPtr {
            limit: (core::mem::size_of_val(&*idt) - 1) as u16,
            #[cfg(target_arch = "x86")]
            base: idt as u32,
            #[cfg(target_arch = "x86_64")]
            base: idt as u64,
        };
        load_idt(&raw const ptr as *const u8);
    }
}

fn code_selector() -> u16 {
    let selector: u16;
    unsafe {
        core::arch::asm!(
            "mov {sel:x}, cs",
            sel = out(reg) selector,
            options(nomem, nostack, preserves_flags)
        );
    }
    selector
}

fn load_idt(descriptor: *const u8) {
    unsafe {
        core::arch::asm!(
            "lidt [{ptr}]",
            ptr = in(reg) descriptor,
            options(readonly, nostack, preserves_flags)
        );
    }
}

/// Invoke native INT 21h AH=30h with the supplied carry bit.
/// Returns the version word, the OEM word, the serial word, and carry.
pub fn get_version(carry: bool) -> (u16, u16, u16, bool) {
    let (version, oem, serial): (u32, u32, u32);
    let flags: u32;
    // The DOS gate preserves all registers other than the three version words.
    // LLVM reserves RBX on x86_64, so save it and copy its DOS result to ESI.
    unsafe {
        #[cfg(target_arch = "x86")]
        core::arch::asm!(
            "bt edx, 0", "int 0x21", "setc dl",
            inout("eax") 0x30a5u32 => version,
            inout("ebx") 0x1234u32 => oem,
            inout("ecx") 0x5678u32 => serial,
            inlateout("edx") u32::from(carry) => flags,
            clobber_abi("C"),
        );
        #[cfg(target_arch = "x86_64")]
        core::arch::asm!(
            "push rbx", "mov ebx, 0x1234", "bt edx, 0", "int 0x21",
            "mov esi, ebx", "pop rbx", "setc dl",
            inout("eax") 0x30a5u32 => version,
            lateout("esi") oem,
            inout("ecx") 0x5678u32 => serial,
            inlateout("edx") u32::from(carry) => flags,
            clobber_abi("C"),
        );
    }
    (version as u16, oem as u16, serial as u16, flags & 1 != 0)
}

pub fn halt() -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

pub fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

pub fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// Read a 16-bit PC I/O port from the kernel's privileged execution context.
pub fn inw(port: u16) -> u16 {
    let value: u16;
    // SAFETY: Both native kernels run at CPL 0. IN only returns the selected port's value.
    unsafe {
        core::arch::asm!("in ax, dx", in("dx") port, out("ax") value,
        options(nomem, nostack, preserves_flags));
    }
    value
}
