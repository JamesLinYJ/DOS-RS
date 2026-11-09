//! Multiboot trampoline and the trap-to-logical mapping.
//!
//! The trampoline is the spike's. `kmain` does not read the registers it pushes.
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
    push ebx
    push eax
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

    pub(crate) fn seal(&self) {
        let _ = self._priv;
    }
}

/// Address of the INT 21h gate, for the IDT slot `kmain` installs.
///
/// The pointer is an integer so `kmain` does not name the trap layout.
pub(crate) fn gate_addr() -> usize {
    int21_gate as *const () as usize
}

/// i686 ring 0 trap. The CPU pushes EFLAGS, CS, and EIP as 4-byte slots.
///
/// GPRs, DS, and ES are software-saved. This struct is not a stack overlay.
#[cfg(target_arch = "x86")]
struct TrapFrame {
    eax: u32,
    ebx: u32,
    ecx: u32,
    edx: u32,
    esi: u32,
    edi: u32,
    ebp: u32,
    ds: u16,
    es: u16,
    eip: u32,
    cs: u16,
    eflags: u32,
}

/// x86_64 trap. The CPU pushes SS, RSP, RFLAGS, CS, and RIP as 8-byte slots.
///
/// GPRs, DS, and ES are software-saved. This struct is not a stack overlay.
#[cfg(target_arch = "x86_64")]
struct TrapFrame {
    rax: u64,
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    rbp: u64,
    ds: u16,
    es: u16,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

impl TrapFrame {
    fn capture() -> Self {
        // TODO: save GPRs, DS, and ES in software.
        // TODO: on i686, read the CPU push of EFLAGS, CS, EIP.
        // TODO: on x86_64, read the CPU push of SS, RSP, RFLAGS, CS, RIP.
        unimplemented!("read the CPU trap frame")
    }
}

/// IDT target for vector 0x21.
///
/// A Rust prologue would clobber the GPRs this mapper needs, so the filled-in
/// gate is a naked stub. It saves GPRs into a `TrapFrame` with `mov`, calls
/// [`dispatch_from_trap`], restores the GPRs, and executes `iret` without
/// writing the pushed flags.
fn int21_gate() {
    let mut trap = TrapFrame::capture();
    dispatch_from_trap(&mut trap);
    // TODO: naked stub. iret. Leave the pushed flags slot as the CPU wrote it.
    unimplemented!("iret")
}

fn dispatch_from_trap(trap: &mut TrapFrame) {
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
            ax: u64::from(trap.eax),
            bx: u64::from(trap.ebx),
            cx: u64::from(trap.ecx),
            dx: u64::from(trap.edx),
            si: u64::from(trap.esi),
            di: u64::from(trap.edi),
            bp: u64::from(trap.ebp),
            ds: u64::from(trap.ds),
            es: u64::from(trap.es),
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
            ax: trap.rax,
            bx: trap.rbx,
            cx: trap.rcx,
            dx: trap.rdx,
            si: trap.rsi,
            di: trap.rdi,
            bp: trap.rbp,
            ds: u64::from(trap.ds),
            es: u64::from(trap.es),
        },
    )
}

/// Writes the logical GPRs back into the trap.
///
/// On i686 each GPR write stores the low 32 bits. On x86_64 it stores the full `u64`.
/// DS and ES are stored as 16-bit selectors. The pushed flags and the return frame are not written.
#[cfg(target_arch = "x86")]
fn logical_into_trap(token: &GateToken, frame: &LogicalFrame, trap: &mut TrapFrame) {
    let regs = frame.native(token);
    trap.eax = regs.ax as u32;
    trap.ebx = regs.bx as u32;
    trap.ecx = regs.cx as u32;
    trap.edx = regs.dx as u32;
    trap.esi = regs.si as u32;
    trap.edi = regs.di as u32;
    trap.ebp = regs.bp as u32;
    trap.ds = regs.ds as u16;
    trap.es = regs.es as u16;
}

/// Writes the logical GPRs back into the trap.
///
/// Does not write RFLAGS, RIP, CS, RSP, or SS.
#[cfg(target_arch = "x86_64")]
fn logical_into_trap(token: &GateToken, frame: &LogicalFrame, trap: &mut TrapFrame) {
    let regs = frame.native(token);
    trap.rax = regs.ax;
    trap.rbx = regs.bx;
    trap.rcx = regs.cx;
    trap.rdx = regs.dx;
    trap.rsi = regs.si;
    trap.rdi = regs.di;
    trap.rbp = regs.bp;
    trap.ds = regs.ds as u16;
    trap.es = regs.es as u16;
}
