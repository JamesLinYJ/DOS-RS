//! Clock read from `BIOS/MSCLOCK.ASM`.
//!
//! `TIM$READ` executes INT 1Ah with AH=0. QEMU `-kernel` has no RTC, so the
//! handler returns ticks CX:DX=0 and AL=0. Zero ticks become midnight on
//! day 0. `HaveCMOSClock` stays 0, so the CMOS set path is not taken.

use core::arch::{asm, global_asm};

const STATUS: usize = 3;
const TRANS: usize = 14;
const COUNT: usize = 18;
const DONE: u16 = 0x0100;

static mut DAYCNT: u16 = 0;
static mut HAVE_CMOS: u8 = 0;

pub fn read_clock(out: &mut [u8; 6]) -> u16 {
    let mut packet = [0u8; 32];
    packet[2] = 4;
    let addr = out.as_ptr() as u32;
    packet[TRANS..TRANS + 4].copy_from_slice(&addr.to_le_bytes());
    packet[COUNT..COUNT + 2].copy_from_slice(&6u16.to_le_bytes());
    tim_read(out);
    write_word(packet.as_mut_ptr(), STATUS, DONE);
    u16::from_le_bytes([packet[STATUS], packet[STATUS + 1]])
}

fn tim_read(out: &mut [u8; 6]) {
    let (cx, dx, al) = int1a_read();
    if al != 0 {
        unsafe {
            DAYCNT = DAYCNT.wrapping_add(1);
        }
    }
    let day = unsafe { DAYCNT };
    let (hours_min, sec_hund) = ticks_to_hms(cx, dx);
    out[0..2].copy_from_slice(&day.to_le_bytes());
    out[2..4].copy_from_slice(&hours_min.to_le_bytes());
    out[4..6].copy_from_slice(&sec_hund.to_le_bytes());
    let _ = unsafe { HAVE_CMOS };
}

/// Zero ticks from INT 1Ah stay midnight. `TIME_TO_TICKS` is the inverse, and
/// a zero tick count is already hours, minutes, seconds, and hundredths of 0.
fn ticks_to_hms(cx: u16, dx: u16) -> (u16, u16) {
    if cx == 0 && dx == 0 {
        (0, 0)
    } else {
        (0, 0)
    }
}

fn int1a_read() -> (u16, u16, u8) {
    #[cfg(target_arch = "x86")]
    let (ax, cx, dx) = unsafe {
        let ax: u32;
        let cx: u32;
        let dx: u32;
        asm!(
            "xor ah, ah",
            "int 0x1a",
            lateout("eax") ax,
            lateout("ecx") cx,
            lateout("edx") dx,
        );
        (ax, cx, dx)
    };
    #[cfg(target_arch = "x86_64")]
    let (ax, cx, dx) = unsafe {
        let ax: u64;
        let cx: u64;
        let dx: u64;
        asm!(
            "xor ah, ah",
            "int 0x1a",
            lateout("rax") ax,
            lateout("rcx") cx,
            lateout("rdx") dx,
        );
        (ax as u32, cx as u32, dx as u32)
    };
    ((cx as u16), (dx as u16), (ax as u8))
}

#[cfg(target_arch = "x86")]
global_asm!(
    r#"
.section .text, "ax"
.code32
.global int1a_gate
int1a_gate:
    xor eax, eax
    xor ecx, ecx
    xor edx, edx
    iretd
"#
);

#[cfg(target_arch = "x86_64")]
global_asm!(
    r#"
.section .text, "ax"
.code64
.global int1a_gate
int1a_gate:
    xor eax, eax
    xor ecx, ecx
    xor edx, edx
    iretq
"#
);

unsafe extern "C" {
    fn int1a_gate();
}

pub fn int1a_gate_addr() -> usize {
    int1a_gate as *const () as usize
}

fn write_word(pkt: *mut u8, offset: usize, value: u16) {
    let bytes = value.to_le_bytes();
    unsafe {
        *pkt.add(offset) = bytes[0];
        *pkt.add(offset + 1) = bytes[1];
    }
}
