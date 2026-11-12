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
    mov edi, eax
    mov esi, ebx
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

/// i686 ring 0 trap. The CPU pushes EFLAGS, CS, and EIP as 4-byte slots.
///
/// GPRs, DS, and ES are software-saved into this `repr(C)` image. The image
/// is not the CPU push itself. The naked gate copies the push in with `mov`.
#[cfg(target_arch = "x86")]
#[repr(C)]
struct TrapFrame {
    eax: u32,
    ebx: u32,
    ecx: u32,
    edx: u32,
    esi: u32,
    edi: u32,
    ebp: u32,
    ds: u32,
    es: u32,
    eip: u32,
    cs: u32,
    eflags: u32,
}

#[cfg(target_arch = "x86")]
const _: () = assert!(core::mem::size_of::<TrapFrame>() == 48);
#[cfg(target_arch = "x86")]
const _: () = assert!(core::mem::offset_of!(TrapFrame, eflags) == 44);

/// x86_64 trap. The CPU pushes SS, RSP, RFLAGS, CS, and RIP as 8-byte slots.
///
/// GPRs, DS, and ES are software-saved into this `repr(C)` image. The image
/// is not the CPU push itself. The naked gate copies the push in with `mov`.
#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct TrapFrame {
    rax: u64,
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    rbp: u64,
    ds: u64,
    es: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

#[cfg(target_arch = "x86_64")]
const _: () = assert!(core::mem::size_of::<TrapFrame>() == 112);
#[cfg(target_arch = "x86_64")]
const _: () = assert!(core::mem::offset_of!(TrapFrame, ss) == 104);

#[cfg(target_arch = "x86")]
global_asm!(
    r#"
.section .text, "ax"
.code32
.global int21_gate
int21_gate:
    sub esp, 48
    mov [esp + 0], eax
    mov [esp + 4], ebx
    mov [esp + 8], ecx
    mov [esp + 12], edx
    mov [esp + 16], esi
    mov [esp + 20], edi
    mov [esp + 24], ebp
    mov eax, [esp + 48]
    mov [esp + 36], eax
    mov eax, [esp + 52]
    mov [esp + 40], eax
    mov eax, [esp + 56]
    mov [esp + 44], eax
    xor eax, eax
    mov ax, ds
    mov [esp + 28], eax
    xor eax, eax
    mov ax, es
    mov [esp + 32], eax
    mov ebx, esp
    and esp, -16
    sub esp, 12
    push ebx
    cld
    call dispatch_from_trap
    mov esp, ebx
    mov eax, [esp + 28]
    mov ds, ax
    mov eax, [esp + 32]
    mov es, ax
    mov ebx, [esp + 4]
    mov ecx, [esp + 8]
    mov edx, [esp + 12]
    mov esi, [esp + 16]
    mov edi, [esp + 20]
    mov ebp, [esp + 24]
    mov eax, [esp + 0]
    add esp, 48
    iretd
"#
);

#[cfg(target_arch = "x86_64")]
global_asm!(
    r#"
.section .text, "ax"
.code64
.global int21_gate
int21_gate:
    sub rsp, 128
    sub rsp, 112
    mov [rsp + 0], rax
    mov [rsp + 8], rbx
    mov [rsp + 16], rcx
    mov [rsp + 24], rdx
    mov [rsp + 32], rsi
    mov [rsp + 40], rdi
    mov [rsp + 48], rbp
    mov rax, [rsp + 240]
    mov [rsp + 72], rax
    mov rax, [rsp + 248]
    mov [rsp + 80], rax
    mov rax, [rsp + 256]
    mov [rsp + 88], rax
    mov rax, [rsp + 264]
    mov [rsp + 96], rax
    mov rax, [rsp + 272]
    mov [rsp + 104], rax
    xor rax, rax
    mov ax, ds
    mov [rsp + 56], rax
    xor rax, rax
    mov ax, es
    mov [rsp + 64], rax
    mov rbx, rsp
    mov rdi, rbx
    push r8
    push r9
    push r10
    push r11
    and rsp, -16
    cld
    call dispatch_from_trap
    mov r8, [rbx-8]
    mov r9, [rbx-16]
    mov r10, [rbx-24]
    mov r11, [rbx-32]
    mov rsp, rbx
    mov ax, [rsp + 56]
    mov ds, ax
    mov ax, [rsp + 64]
    mov es, ax
    mov rbx, [rsp + 8]
    mov rcx, [rsp + 16]
    mov rdx, [rsp + 24]
    mov rsi, [rsp + 32]
    mov rdi, [rsp + 40]
    mov rbp, [rsp + 48]
    mov rax, [rsp + 0]
    add rsp, 240
    iretq
"#
);

#[unsafe(no_mangle)]
unsafe extern "C" fn dispatch_from_trap(trap: *mut TrapFrame) {
    let trap = unsafe { &mut *trap };
    let token = GateToken::mint();
    let mut logical = logical_from_trap(&token, trap);
    int21::entry(&token, &mut logical);
    logical_into_trap(&token, &logical, trap);
}

/// Copies the trap into a [`LogicalFrame`].
///
/// i686 GPRs are zero-extended into `u64`. x86_64 GPRs are copied whole.
/// DS and ES are copied. EFLAGS, RFLAGS, the return address, CS, and the x86_64
/// interrupt SS and RSP are not copied.
#[cfg(target_arch = "x86")]
fn logical_from_trap(token: &GateToken, trap: &TrapFrame) -> LogicalFrame {
    let _flags_stay = trap.eflags;
    let _eip_stays = trap.eip;
    let _cs_stays = trap.cs;
    LogicalFrame::from_native(
        token,
        NativeRegs {
            status: u64::from(trap.eax),
            handle: u64::from(trap.ebx),
            count: u64::from(trap.ecx),
            buffer: u64::from(trap.edx),
            source: u64::from(trap.esi),
            destination: u64::from(trap.edi),
            base: u64::from(trap.ebp),
            data_segment: u64::from(trap.ds),
            extra_segment: u64::from(trap.es),
        },
    )
}

/// Copies the trap into a [`LogicalFrame`].
///
/// GPRs are copied as `u64`. DS and ES are zero-extended. RFLAGS, RIP, CS, RSP, and SS stay in the trap.
#[cfg(target_arch = "x86_64")]
fn logical_from_trap(token: &GateToken, trap: &TrapFrame) -> LogicalFrame {
    let _flags_stay = trap.rflags;
    let _rip_stays = trap.rip;
    let _cs_stays = trap.cs;
    let _rsp_stays = trap.rsp;
    let _ss_stays = trap.ss;
    LogicalFrame::from_native(
        token,
        NativeRegs {
            status: trap.rax,
            handle: trap.rbx,
            count: trap.rcx,
            buffer: trap.rdx,
            source: trap.rsi,
            destination: trap.rdi,
            base: trap.rbp,
            data_segment: trap.ds,
            extra_segment: trap.es,
        },
    )
}

/// Writes the logical GPRs back into the trap.
///
/// On i686 each GPR write stores the low 32 bits. On x86_64 it stores the full `u64`.
/// DS and ES are written back as selector values. The pushed flags are not written.
#[cfg(target_arch = "x86")]
fn logical_into_trap(token: &GateToken, frame: &LogicalFrame, trap: &mut TrapFrame) {
    let regs = frame.native(token);
    trap.eax = regs.status as u32;
    trap.ebx = regs.handle as u32;
    trap.ecx = regs.count as u32;
    trap.edx = regs.buffer as u32;
    trap.esi = regs.source as u32;
    trap.edi = regs.destination as u32;
    trap.ebp = regs.base as u32;
    trap.ds = regs.data_segment as u32;
    trap.es = regs.extra_segment as u32;
}

/// Writes the logical GPRs back into the trap.
///
/// Does not write RFLAGS, RIP, CS, RSP, or SS.
#[cfg(target_arch = "x86_64")]
fn logical_into_trap(token: &GateToken, frame: &LogicalFrame, trap: &mut TrapFrame) {
    let regs = frame.native(token);
    trap.rax = regs.status;
    trap.rbx = regs.handle;
    trap.rcx = regs.count;
    trap.rdx = regs.buffer;
    trap.rsi = regs.source;
    trap.rdi = regs.destination;
    trap.rbp = regs.base;
    trap.ds = regs.data_segment;
    trap.es = regs.extra_segment;
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
    #[cfg(target_arch = "x86")]
    let (version, oem, serial, flags) = unsafe {
        let version: u32;
        let oem: u32;
        let serial: u32;
        let flags: u32;
        core::arch::asm!(
            "bt edx, 0",
            "mov eax, 0x30a5",
            "mov ebx, 0x1234",
            "mov ecx, 0x5678",
            "int 0x21",
            "pushfd",
            "pop edx",
            lateout("eax") version,
            lateout("ebx") oem,
            lateout("ecx") serial,
            inlateout("edx") u32::from(carry) => flags,
            clobber_abi("C"),
        );
        (version, oem, serial, flags)
    };
    #[cfg(target_arch = "x86_64")]
    let (version, oem, serial, flags) = unsafe {
        let version: u64;
        let oem: u64;
        let serial: u64;
        let flags: u64;
        core::arch::asm!(
            "bt edx, 0",
            "push rbx",
            "mov eax, 0x30a5",
            "mov ebx, 0x1234",
            "mov ecx, 0x5678",
            "int 0x21",
            "mov r8, rax",
            "mov r9, rcx",
            "pushfq",
            "pop r10",
            "mov rax, rbx",
            "pop rbx",
            lateout("r8") version,
            lateout("rax") oem,
            lateout("r9") serial,
            lateout("r10") flags,
            in("edx") u32::from(carry),
            clobber_abi("C"),
        );
        (version, oem, serial, flags)
    };
    (version as u16, oem as u16, serial as u16, flags & 1 == 1)
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
