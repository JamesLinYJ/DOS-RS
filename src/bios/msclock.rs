//! Convert between BIOS ticks and the six-byte DOS clock record.

use core::sync::atomic::{AtomicU16, Ordering};

const DONE: u16 = 0x0100;
const TICK_NUMERATOR: u64 = 59_659;
const TICK_DENOMINATOR: u64 = 5 * 65_536;
static DAY: AtomicU16 = AtomicU16::new(0);

pub fn read_clock(out: &mut [u8; 6]) -> u16 {
    read_clock_at(out, 0, 0)
}

pub fn read_clock_at(out: &mut [u8; 6], ticks_hi: u16, ticks_lo: u16) -> u16 {
    let ticks = (u32::from(ticks_hi) << 16) | u32::from(ticks_lo);
    // TIM$READ multiplies the BIOS counter in 32 bits and keeps 17 bits of seconds.
    let hundredths = (u64::from(ticks.wrapping_mul(5)) << 16) / TICK_NUMERATOR;
    let seconds = (hundredths / 100) & 0x1ffff;
    let [day_lo, day_hi] = DAY.load(Ordering::Relaxed).to_le_bytes();
    *out = [
        day_lo,
        day_hi,
        (seconds / 60 % 60) as u8,
        (seconds / 3600) as u8,
        (hundredths % 100) as u8,
        (seconds % 60) as u8,
    ];
    DONE
}

pub fn write_clock(time: &[u8; 6]) -> (u16, u16) {
    let [day_lo, day_hi, minutes, hours, hundredths, seconds] = *time;
    DAY.store(u16::from_le_bytes([day_lo, day_hi]), Ordering::Relaxed);
    let minutes = u64::from(hours) * 60 + u64::from(minutes);
    let elapsed = (minutes * 60 + u64::from(seconds)) * 100 + u64::from(hundredths);
    let ticks = elapsed * TICK_NUMERATOR / TICK_DENOMINATOR;
    (((ticks >> 16) & 0xff) as u16, ticks as u16)
}
