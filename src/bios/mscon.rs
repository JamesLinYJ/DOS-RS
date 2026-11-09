//! Console driver from `BIOS/MSCON.ASM`, plus `OUTCHR` from `BIOS/MSBIO1.ASM`.
//!
//! `CON$WRIT` executes `int 0x29` once per byte. `OUTCHR` is that interrupt.
//! It executes `int 0x10` with AH=`0x0E` and BX=7. There is no VGA BIOS under
//! QEMU `-kernel`, so AH=`0x0E` writes AL to COM1. Other AH values return.
//! There is no ROM keyboard, so `int 0x16` returns AX=0 with ZF set.

use core::arch::{asm, global_asm};

const CMDLEN: usize = 0;
const UNIT: usize = 1;
const CMD: usize = 2;
const STATUS: usize = 3;
const MEDIA: usize = 13;
const TRANS: usize = 14;
const COUNT: usize = 18;

const DEV_READ: u8 = 4;
const DEV_RDND: u8 = 5;
const DEV_FLUSH: u8 = 7;
const DEV_WRITE: u8 = 8;
const DEV_WRITE_VERIFY: u8 = 9;

const DONE: u16 = 0x0100;
const BUSY_DONE: u16 = 0x0300;
const ERROR_DONE: u16 = 0x8100;
const UNKNOWN_COMMAND: u8 = 3;
const SYSTEM_WAIT: u16 = 0x0400;

struct ConState {
    altah: u8,
    keyrd_func: u8,
    keysts_func: u8,
    fhavek09: u8,
    ptrsav: usize,
}

static mut CON: ConState = ConState {
    altah: 0,
    keyrd_func: 0,
    keysts_func: 1,
    fhavek09: 0,
    ptrsav: 0,
};

/// Maximum command in `CONTBL` (`DB 10`), then one slot per command.
const CONTBL: [fn(); 11] = [
    exit_ok,
    exit_ok,
    exit_ok,
    cmd_err,
    con_read,
    con_rdnd,
    exit_ok,
    con_flush,
    con_writ,
    con_writ,
    exit_ok,
];

const _: () = assert!(CONTBL.len() - 1 == 10);

pub fn con_write_bytes(bytes: &[u8]) {
    let mut packet = [0u8; 32];
    packet[CMDLEN] = 22;
    packet[CMD] = DEV_WRITE;
    let addr = bytes.as_ptr() as u32;
    packet[TRANS..TRANS + 4].copy_from_slice(&addr.to_le_bytes());
    let count = bytes.len() as u16;
    packet[COUNT..COUNT + 2].copy_from_slice(&count.to_le_bytes());
    unsafe {
        (*core::ptr::addr_of_mut!(CON)).ptrsav = packet.as_mut_ptr() as usize;
    }
    dispatch();
}

fn dispatch() {
    let cmd = packet_byte(CMD);
    if cmd as usize >= CONTBL.len() {
        cmd_err();
        return;
    }
    CONTBL[cmd as usize]();
}

fn con_read() {
    let count = packet_word(COUNT);
    if count == 0 {
        exit_ok();
        return;
    }
    let dest = trans_ptr() as *mut u8;
    let mut left = count;
    while left != 0 {
        let al = chrin();
        unsafe {
            dest.add((count - left) as usize).write(al);
        }
        left -= 1;
    }
    exit_ok();
}

fn chrin() -> u8 {
    loop {
        let func = unsafe { (*core::ptr::addr_of!(CON)).keyrd_func };
        let held = unsafe {
            let state = &mut *core::ptr::addr_of_mut!(CON);
            let al = state.altah;
            state.altah = 0;
            al
        };
        if held != 0 {
            return held;
        }
        let (ax, _zf) = int16(func);
        if ax == 0 {
            continue;
        }
        if ax == 0x7200 {
            return 16;
        }
        let al = ax as u8;
        let ah = (ax >> 8) as u8;
        if func != 0 && al == 0xE0 {
            if ah == 0 {
                return al;
            }
            unsafe {
                (*core::ptr::addr_of_mut!(CON)).altah = ah;
            }
            return 0;
        }
        if al != 0 {
            return al;
        }
        unsafe {
            (*core::ptr::addr_of_mut!(CON)).altah = ah;
        }
        return 0;
    }
}

fn con_rdnd() {
    let held = unsafe { (*core::ptr::addr_of!(CON)).altah };
    if held != 0 {
        rd_exit(held);
        return;
    }
    let func = unsafe { (*core::ptr::addr_of!(CON)).keysts_func };
    let (ax, zf) = int16(func);
    if zf {
        no_char();
        return;
    }
    if ax == 0 {
        let read_func = unsafe { (*core::ptr::addr_of!(CON)).keyrd_func };
        let _ = int16(read_func);
        con_rdnd();
        return;
    }
    if ax == 0x7200 {
        rd_exit(16);
        return;
    }
    let mut al = ax as u8;
    let ah = (ax >> 8) as u8;
    let extended = unsafe { (*core::ptr::addr_of!(CON)).keyrd_func } != 0;
    if extended && al == 0xE0 && ah != 0 {
        al = 0;
    }
    rd_exit(al);
}

fn no_char() {
    let k09 = unsafe { (*core::ptr::addr_of!(CON)).fhavek09 };
    if k09 == 0 {
        bus_exit();
        return;
    }
    let status = packet_word(STATUS);
    if status & SYSTEM_WAIT == 0 {
        bus_exit();
        return;
    }
    bus_exit();
}

fn rd_exit(al: u8) {
    let pkt = ptrsav();
    unsafe {
        *pkt.add(MEDIA) = al;
    }
    exit_with(al);
}

fn con_flush() {
    unsafe {
        (*core::ptr::addr_of_mut!(CON)).altah = 0;
    }
    loop {
        let func = unsafe { (*core::ptr::addr_of!(CON)).keysts_func };
        let (_ax, zf) = int16(func);
        if zf {
            break;
        }
        let read_func = unsafe { (*core::ptr::addr_of!(CON)).keyrd_func };
        let _ = int16(read_func);
    }
    exit_ok();
}

fn con_writ() {
    let count = packet_word(COUNT);
    if count == 0 {
        exit_ok();
        return;
    }
    let src = trans_ptr();
    let mut left = count;
    while left != 0 {
        let al = unsafe { *src.add((count - left) as usize) };
        int29(al);
        left -= 1;
    }
    exit_ok();
}

fn exit_ok() {
    exit_with(0);
}

fn cmd_err() {
    let pkt = ptrsav();
    let count = packet_word(COUNT);
    write_word(pkt, COUNT, 0u16.wrapping_sub(count));
    write_word(pkt, STATUS, ERROR_DONE | UNKNOWN_COMMAND as u16);
}

fn bus_exit() {
    let pkt = ptrsav();
    write_word(pkt, STATUS, BUSY_DONE);
}

fn exit_with(al: u8) {
    let pkt = ptrsav();
    write_word(pkt, STATUS, DONE | al as u16);
}

fn ptrsav() -> *mut u8 {
    unsafe { (*core::ptr::addr_of!(CON)).ptrsav as *mut u8 }
}

fn packet_byte(offset: usize) -> u8 {
    unsafe { *ptrsav().add(offset) }
}

fn packet_word(offset: usize) -> u16 {
    let pkt = ptrsav();
    unsafe { u16::from_le_bytes([*pkt.add(offset), *pkt.add(offset + 1)]) }
}

fn write_word(pkt: *mut u8, offset: usize, value: u16) {
    let bytes = value.to_le_bytes();
    unsafe {
        *pkt.add(offset) = bytes[0];
        *pkt.add(offset + 1) = bytes[1];
    }
}

fn trans_ptr() -> *const u8 {
    let pkt = ptrsav();
    let mut raw = [0u8; 4];
    unsafe {
        raw[0] = *pkt.add(TRANS);
        raw[1] = *pkt.add(TRANS + 1);
        raw[2] = *pkt.add(TRANS + 2);
        raw[3] = *pkt.add(TRANS + 3);
    }
    u32::from_le_bytes(raw) as *const u8
}

fn int29(al: u8) {
    unsafe {
        asm!(
            "mov al, {al}",
            "int 0x29",
            al = in(reg_byte) al,
            lateout("eax") _,
        );
    }
}

fn int16(ah: u8) -> (u16, bool) {
    #[cfg(target_arch = "x86")]
    let (ax, flags) = unsafe {
        let ax: u32;
        let flags: u32;
        asm!(
            "mov ah, {ah}",
            "int 0x16",
            "pushfd",
            "pop edx",
            ah = in(reg_byte) ah,
            lateout("eax") ax,
            lateout("edx") flags,
        );
        (ax, flags)
    };
    #[cfg(target_arch = "x86_64")]
    let (ax, flags) = unsafe {
        let ax: u64;
        let flags: u64;
        asm!(
            "mov ah, {ah}",
            "int 0x16",
            "pushfq",
            "pop rdx",
            ah = in(reg_byte) ah,
            lateout("rax") ax,
            lateout("rdx") flags,
        );
        (ax, flags)
    };
    ((ax as u16), (flags & 0x40) != 0)
}

#[no_mangle]
extern "C" fn bios_outchr(al: usize) {
    let al = al as u8;
    unsafe {
        #[cfg(target_arch = "x86")]
        asm!(
            "mov al, {al}",
            "mov ah, 0x0e",
            "push ebx",
            "mov bx, 7",
            "int 0x10",
            "pop ebx",
            al = in(reg_byte) al,
            lateout("eax") _,
        );
        #[cfg(target_arch = "x86_64")]
        asm!(
            "mov al, {al}",
            "mov ah, 0x0e",
            "push rbx",
            "mov bx, 7",
            "int 0x10",
            "pop rbx",
            al = in(reg_byte) al,
            lateout("rax") _,
        );
    }
}

#[no_mangle]
extern "C" fn bios_int10(ax: usize) {
    let ah = ((ax >> 8) & 0xff) as u8;
    let al = (ax & 0xff) as u8;
    if ah == 0x0e {
        crate::serial_putc(al);
    }
}

#[cfg(target_arch = "x86")]
global_asm!(
    r#"
.section .text, "ax"
.code32
.global int29_gate
int29_gate:
    push eax
    push ecx
    push edx
    movzx ecx, al
    push ecx
    call bios_outchr
    add esp, 4
    pop edx
    pop ecx
    pop eax
    iretd
.global int10_gate
int10_gate:
    push eax
    push ecx
    push edx
    movzx ecx, ax
    push ecx
    call bios_int10
    add esp, 4
    pop edx
    pop ecx
    pop eax
    iretd
.global int16_gate
int16_gate:
    xor eax, eax
    or dword ptr [esp + 8], 0x40
    iretd
"#
);

#[cfg(target_arch = "x86_64")]
global_asm!(
    r#"
.section .text, "ax"
.code64
.global int29_gate
int29_gate:
    push rax
    push rcx
    push rdx
    push rbx
    movzx rdi, al
    mov rbx, rsp
    and rsp, -16
    call bios_outchr
    mov rsp, rbx
    pop rbx
    pop rdx
    pop rcx
    pop rax
    iretq
.global int10_gate
int10_gate:
    push rax
    push rcx
    push rdx
    push rbx
    movzx rdi, ax
    mov rbx, rsp
    and rsp, -16
    call bios_int10
    mov rsp, rbx
    pop rbx
    pop rdx
    pop rcx
    pop rax
    iretq
.global int16_gate
int16_gate:
    xor eax, eax
    or qword ptr [rsp + 16], 0x40
    iretq
"#
);

unsafe extern "C" {
    fn int29_gate();
    fn int10_gate();
    fn int16_gate();
}

pub fn int29_gate_addr() -> usize {
    int29_gate as *const () as usize
}

pub fn int10_gate_addr() -> usize {
    int10_gate as *const () as usize
}

pub fn int16_gate_addr() -> usize {
    int16_gate as *const () as usize
}

#[allow(dead_code)]
fn _command_names() {
    let _ = (DEV_READ, DEV_RDND, DEV_FLUSH, DEV_WRITE_VERIFY, UNIT);
}
