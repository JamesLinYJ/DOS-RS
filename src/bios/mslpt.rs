//! Printer write from `BIOS/MSLPT.ASM`.
//!
//! `PRN$WRIT` calls INT 17h with AH=0. There is no printer ROM under QEMU
//! `-kernel`. The handler writes AL to COM1 and returns AH=0, so the timeout
//! bit is clear and the write continues.

use core::arch::{asm, global_asm};

const STATUS: usize = 3;
const TRANS: usize = 14;
const COUNT: usize = 18;
const DONE: u16 = 0x0100;
const IOERR: u8 = 0x08;
const TIMEOUT: u8 = 0x01;

pub fn prn_write_bytes(bytes: &[u8]) -> u16 {
    let mut packet = [0u8; 32];
    let mut left = bytes.len() as u16;
    let mut index = 0usize;
    while left != 0 {
        let mut retries = 2u8;
        loop {
            let ah = int17_print(bytes[index]);
            if ah & IOERR == 0 && ah & TIMEOUT == 0 {
                break;
            }
            retries = retries.saturating_sub(1);
            if retries == 0 {
                write_word(packet.as_mut_ptr(), STATUS, 0x8100 | 10);
                return 0x810A;
            }
        }
        index += 1;
        left -= 1;
    }
    write_word(packet.as_mut_ptr(), STATUS, DONE);
    let _ = (COUNT, TRANS);
    DONE
}

fn int17_print(al: u8) -> u8 {
    #[cfg(target_arch = "x86")]
    let ax = unsafe {
        let ax: u32;
        asm!(
            "mov al, {byte}",
            "xor ah, ah",
            "int 0x17",
            byte = in(reg_byte) al,
            lateout("eax") ax,
        );
        ax
    };
    #[cfg(target_arch = "x86_64")]
    let ax = unsafe {
        let ax: u64;
        asm!(
            "mov al, {byte}",
            "xor ah, ah",
            "int 0x17",
            byte = in(reg_byte) al,
            lateout("rax") ax,
        );
        ax as u32
    };
    ((ax >> 8) & 0xff) as u8
}

#[no_mangle]
extern "C" fn bios_int17(ax: usize) {
    let ah = ((ax >> 8) & 0xff) as u8;
    let al = (ax & 0xff) as u8;
    if ah == 0 {
        crate::serial_putc(al);
    }
}

#[cfg(target_arch = "x86")]
global_asm!(
    r#"
.section .text, "ax"
.code32
.global int17_gate
int17_gate:
    push eax
    push ecx
    push edx
    movzx ecx, ax
    push ecx
    call bios_int17
    add esp, 4
    pop edx
    pop ecx
    pop eax
    xor ah, ah
    iretd
"#
);

#[cfg(target_arch = "x86_64")]
global_asm!(
    r#"
.section .text, "ax"
.code64
.global int17_gate
int17_gate:
    push rax
    push rcx
    push rdx
    push rbx
    movzx rdi, ax
    mov rbx, rsp
    and rsp, -16
    call bios_int17
    mov rsp, rbx
    pop rbx
    pop rdx
    pop rcx
    pop rax
    xor ah, ah
    iretq
"#
);

unsafe extern "C" {
    fn int17_gate();
}

pub fn int17_gate_addr() -> usize {
    int17_gate as *const () as usize
}

fn write_word(pkt: *mut u8, offset: usize, value: u16) {
    let bytes = value.to_le_bytes();
    unsafe {
        *pkt.add(offset) = bytes[0];
        *pkt.add(offset + 1) = bytes[1];
    }
}
