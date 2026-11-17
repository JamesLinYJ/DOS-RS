//! DOS date and time. BIOS ticks and the battery-backed clock agree after a set.
use dosrs_realmode::{
    self as pc,
    system::{sw, w},
};

fn bcd(v: u16) -> u16 {
    (v / 10) * 16 + v % 10
}
fn decimal(v: u16) -> u16 {
    (v >> 4) * 10 + (v & 15)
}
// The 286 backend can divide words. Use unsigned long division for BIOS ticks.
fn divide(n: u32, d: u32) -> u32 {
    let mut q = 0;
    let mut rem = 0;
    let mut bit = 32;
    while bit > 0 {
        bit -= 1;
        rem = (rem << 1) | ((n >> bit) & 1);
        if rem >= d {
            rem -= d;
            q |= 1 << bit
        }
    }
    q
}
fn month_days(y: u16, m: u16) -> u16 {
    match m {
        2 => {
            if y & 3 == 0 {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    }
}
fn date_ok(y: u16, m: u16, d: u16) -> bool {
    y >= 1980 && y <= 2099 && d != 0 && d <= month_days(y, m)
}
fn weekday(y: u16, m: u16, d: u16) -> u16 {
    let yrs = y - 1980;
    let mut days = yrs * 365 + (yrs + 3) / 4 + d + 1;
    let mut i = 1;
    while i < m {
        days += month_days(y, i);
        i += 1
    }
    days % 7
}
pub fn service(ss: u16, sp: u16, ah: u16) -> u32 {
    let mut regs = [0u16; 2];
    let cx = w(ss, sp + 4);
    let dx = w(ss, sp + 6);
    match ah {
        0x2a => {
            if !pc::rtc(0x400, &mut regs) {
                return 255;
            }
            let year = decimal(regs[0] >> 8) * 100 + decimal(regs[0] & 255);
            let month = decimal(regs[1] >> 8);
            let day = decimal(regs[1] & 255);
            if !date_ok(year, month, day) {
                return 255;
            }
            sw(ss, sp + 4, year);
            sw(ss, sp + 6, (month << 8) | day);
            u32::from(weekday(year, month, day))
        }
        0x2b => {
            if !date_ok(cx, dx >> 8, dx & 255) {
                return 255;
            }
            regs[0] = bcd(cx / 100) << 8 | bcd(cx % 100);
            regs[1] = bcd(dx >> 8) << 8 | bcd(dx & 255);
            if pc::rtc(0x500, &mut regs) { 0 } else { 255 }
        }
        0x2c => {
            if !pc::rtc(0, &mut regs) {
                return 255;
            }
            let ticks = (u32::from(regs[0]) << 16) | u32::from(regs[1]);
            // 1,573,040 ticks per day = 19,663 ticks per 108,000 hundredths.
            let blocks = divide(ticks, 19663);
            let cs = blocks * 108000 + divide((ticks - blocks * 19663) * 108000, 19663);
            let h = divide(cs, 360000);
            let leftover = cs - h * 360000;
            let m = divide(leftover, 6000);
            let leftover = leftover - m * 6000;
            let s = divide(leftover, 100);
            sw(ss, sp + 4, ((h as u16) << 8) | m as u16);
            sw(ss, sp + 6, ((s as u16) << 8) | (leftover - s * 100) as u16);
            0
        }
        _ => {
            let h = cx >> 8;
            let m = cx & 255;
            let s = dx >> 8;
            let cs = dx & 255;
            if h >= 24 || m >= 60 || s >= 60 || cs >= 100 {
                return 255;
            }
            regs[0] = bcd(h) << 8 | bcd(m);
            regs[1] = bcd(s) << 8;
            if !pc::rtc(0x300, &mut regs) {
                return 255;
            }
            let total =
                ((u32::from(h) * 60 + u32::from(m)) * 60 + u32::from(s)) * 100 + u32::from(cs);
            let blocks = divide(total, 108000);
            let ticks = blocks * 19663 + divide((total - blocks * 108000) * 19663, 108000);
            regs[0] = (ticks >> 16) as u16;
            regs[1] = ticks as u16;
            if pc::rtc(0x100, &mut regs) { 0 } else { 255 }
        }
    }
}
