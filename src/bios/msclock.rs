//! Clock read from `BIOS/MSCLOCK.ASM`.
//!
//! `TIM$READ` issues INT 1Ah with AH=0. The gate returns the tick count it
//! was given in CX:DX and clears AL, so a day does not roll. Zero ticks are
//! midnight on day 0. A non-zero count uses the same divide sequence as
//! `TIM$READ`: multiply by 5, divide by 59659, then split the hundredths.
//! `HaveCMOSClock` stays 0, so the CMOS set path is not taken.
//!
//! The stored order matches the `TIM$WRIT` loads, which is what `stosw`
//! writes. Byte 2 is minutes, byte 3 is hours, byte 4 is hundredths, and
//! byte 5 is seconds.

use core::arch::{asm, global_asm};

const STATUS: usize = 3;
const TRANS: usize = 14;
const COUNT: usize = 18;
const DONE: u16 = 0x0100;

static mut DAYCNT: u16 = 0;
static mut HAVE_CMOS: u8 = 0;

pub fn read_clock(out: &mut [u8; 6]) -> u16 {
    read_clock_at(out, 0, 0)
}

/// Read the clock after forcing INT 1Ah to the given CX:DX tick count.
pub fn read_clock_at(out: &mut [u8; 6], ticks_hi: u16, ticks_lo: u16) -> u16 {
    let mut packet = [0u8; 32];
    packet[2] = 4;
    let addr = out.as_ptr() as u32;
    packet[TRANS..TRANS + 4].copy_from_slice(&addr.to_le_bytes());
    packet[COUNT..COUNT + 2].copy_from_slice(&6u16.to_le_bytes());
    tim_read(out, ticks_hi, ticks_lo);
    write_word(packet.as_mut_ptr(), STATUS, DONE);
    u16::from_le_bytes([packet[STATUS], packet[STATUS + 1]])
}

fn tim_read(out: &mut [u8; 6], ticks_hi: u16, ticks_lo: u16) {
    let (cx, dx, al) = int1a_read(ticks_hi, ticks_lo);
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

/// `TIM$WRIT`. The six bytes are the `TIM$READ` order. `HaveCMOSClock` is 0,
/// so the BCD set-time and set-date calls are not taken. `TIME_TO_TICKS`
/// still runs, then INT 1Ah AH=1, then `DAYCNT` is stored.
pub fn write_clock(time: &[u8; 6]) -> (u16, u16) {
    let day = u16::from_le_bytes([time[0], time[1]]);
    let minutes = time[2];
    let hours = time[3];
    let hundredths = time[4];
    let seconds = time[5];
    let cmos = unsafe { HAVE_CMOS };
    if cmos != 0 {
        let _hours_bcd = hours;
        let _minutes_bcd = minutes;
        let _seconds_bcd = seconds;
    }
    let (hi, lo) = time_to_ticks(hours, minutes, seconds, hundredths);
    int1a_write(hi, lo);
    unsafe {
        DAYCNT = day;
    }
    if cmos != 0 {
        let _day = day;
    }
    (hi, lo)
}

/// `TIME_TO_TICKS`. CX on entry is minutes in CL and hours in CH. DX is
/// hundredths in DL and seconds in DH. The tick count comes back in CX:DX.
fn time_to_ticks(hours: u8, minutes: u8, seconds: u8, hundredths: u8) -> (u16, u16) {
    let mut cx = u32::from(minutes);
    let mut dx = (u32::from(seconds) << 8) | u32::from(hundredths);
    let mut ax = (60u32 * u32::from(hours)) & 0xffff;
    ax = (ax + cx) & 0xffff;
    cx = 6000;
    let mut bx = dx;
    let prod = ax * cx;
    dx = (prod >> 16) & 0xffff;
    ax = prod & 0xffff;
    cx = ax;
    ax = (100 * ((bx >> 8) & 0xff)) & 0xffff;
    let sum = cx + ax;
    cx = sum & 0xffff;
    dx = (dx + (sum >> 16)) & 0xffff;
    bx &= 0x00ff;
    let sum = cx + bx;
    cx = sum & 0xffff;
    dx = (dx + (sum >> 16)) & 0xffff;
    let old_ax = ax;
    ax = dx;
    dx = old_ax;
    let old_ax = ax;
    ax = cx;
    cx = old_ax;
    bx = 59659;
    let prod = ax * bx;
    dx = (prod >> 16) & 0xffff;
    ax = prod & 0xffff;
    let old_dx = dx;
    dx = cx;
    cx = old_dx;
    let old_ax = ax;
    ax = dx;
    dx = old_ax;
    let prod = ax * bx;
    let high = (prod >> 16) & 0xffff;
    ax = prod & 0xffff;
    let sum = ax + cx;
    ax = sum & 0xffff;
    dx = (high + (sum >> 16)) & 0xffff;
    let old_ax = ax;
    ax = dx;
    dx = old_ax;
    let quot = (ax / 5) & 0xff;
    let rem = ax % 5;
    cx = quot;
    ax = dx;
    dx = rem;
    let dividend = (dx << 16) | ax;
    ax = (dividend / 5) & 0xffff;
    dx = ax;
    (cx as u16, dx as u16)
}

/// `TIM$READ` from ticks in CX:DX to the AX:BX time word pair.
///
/// CX=1 and DX=0x0E is 65550 ticks, which this sequence turns into
/// 1:00:00.36. The six clock bytes are then `00 00 00 01 24 00`.
fn ticks_to_hms(ticks_hi: u16, ticks_lo: u16) -> (u16, u16) {
    let mut cx = u32::from(ticks_hi);
    let mut dx = u32::from(ticks_lo);
    let orig_ax = cx;
    let orig_dx = dx;
    let mut carry = dx >> 15;
    dx = (dx << 1) & 0xffff;
    cx = ((cx << 1) | carry) & 0xffff;
    carry = dx >> 15;
    dx = (dx << 1) & 0xffff;
    cx = ((cx << 1) | carry) & 0xffff;
    let sum = dx + orig_dx;
    carry = sum >> 16;
    dx = sum & 0xffff;
    let ax = (orig_ax + cx + carry) & 0xffff;
    let dividend = (u64::from(ax) << 16) | u64::from(dx);
    let quot = (dividend / 59659) as u32;
    let rem = (dividend % 59659) as u32;
    let low = ((u64::from(rem) << 16) / 59659) as u32;
    let hund = (u64::from(quot) << 16) | u64::from(low);
    let q = (hund / 200) as u32;
    let remainder = (hund % 200) as u32;
    let (hundredths, carry_in) = if remainder < 100 {
        (remainder, 0u32)
    } else {
        (remainder - 100, 1u32)
    };
    let new_ax = ((q << 1) | carry_in) & 0xffff;
    let carry_out = (q >> 15) & 1;
    let seconds_total = (u32::from(carry_out) << 16) | new_ax;
    let minutes_total = seconds_total / 60;
    let seconds = (seconds_total % 60) as u16;
    let hours = (minutes_total / 60) as u16;
    let minutes = (minutes_total % 60) as u16;
    let ax_word = minutes | (hours << 8);
    let bx_word = (hundredths as u16) | (seconds << 8);
    (ax_word, bx_word)
}

fn int1a_write(ticks_hi: u16, ticks_lo: u16) {
    #[cfg(target_arch = "x86")]
    unsafe {
        let mut ax: u16 = 0x0100;
        let mut cx = ticks_hi;
        let mut dx = ticks_lo;
        asm!(
            "int 0x1a",
            inout("ax") ax,
            inout("cx") cx,
            inout("dx") dx,
        );
        let _ = (ax, cx, dx);
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let mut ax: u16 = 0x0100;
        let mut cx = ticks_hi;
        let mut dx = ticks_lo;
        asm!(
            "int 0x1a",
            inout("ax") ax,
            inout("cx") cx,
            inout("dx") dx,
        );
        let _ = (ax, cx, dx);
    }
}

fn int1a_read(ticks_hi: u16, ticks_lo: u16) -> (u16, u16, u8) {
    #[cfg(target_arch = "x86")]
    let (ax, cx, dx) = unsafe {
        let mut ax: u16 = 0;
        let mut cx: u16 = ticks_hi;
        let mut dx: u16 = ticks_lo;
        asm!(
            "xor ah, ah",
            "int 0x1a",
            inout("ax") ax,
            inout("cx") cx,
            inout("dx") dx,
        );
        (ax, cx, dx)
    };
    #[cfg(target_arch = "x86_64")]
    let (ax, cx, dx) = unsafe {
        let mut ax: u16 = 0;
        let mut cx: u16 = ticks_hi;
        let mut dx: u16 = ticks_lo;
        asm!(
            "xor ah, ah",
            "int 0x1a",
            inout("ax") ax,
            inout("cx") cx,
            inout("dx") dx,
        );
        (ax, cx, dx)
    };
    (cx, dx, ax as u8)
}

#[cfg(target_arch = "x86")]
global_asm!(
    r#"
.section .text, "ax"
.code32
.global int1a_gate
int1a_gate:
    xor al, al
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
    xor al, al
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
