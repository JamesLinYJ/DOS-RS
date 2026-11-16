//! Read a consistent wall clock from the PC's battery-backed RTC.
use crate::arch::{inb, outb};
use core::time::Duration;

fn reg(index: u8) -> u8 {
    outb(0x70, index);
    inb(0x71)
}

fn sample() -> Option<[u8; 8]> {
    if reg(0x0a) & 0x80 != 0 {
        return None;
    }
    Some([
        reg(0),
        reg(2),
        reg(4),
        reg(7),
        reg(8),
        reg(9),
        reg(0x32),
        reg(0x0b),
    ])
}
pub(crate) fn now() -> Duration {
    let mut last = None;
    for _ in 0..4096 {
        let sample = sample();
        if sample.is_some() && sample == last {
            let [s, m, h, day, month, y, century, mode] = sample.unwrap();
            let decode = |b: u8| -> Option<u16> {
                if mode & 4 != 0 {
                    Some(u16::from(b))
                } else if b & 15 < 10 && b >> 4 < 10 {
                    Some(u16::from(b >> 4) * 10 + u16::from(b & 15))
                } else {
                    None
                }
            };
            let [
                Some(sec),
                Some(min),
                Some(mut hour),
                Some(d),
                Some(mo),
                Some(yr),
            ] = [s, m, h & 0x7f, day, month, y].map(decode)
            else {
                continue;
            };
            if mode & 2 == 0 {
                if !(1..=12).contains(&hour) {
                    continue;
                }
                hour = hour % 12 + if h & 0x80 != 0 { 12 } else { 0 };
            }
            let century = decode(century)
                .filter(|c| (19..=21).contains(c))
                .unwrap_or(if yr >= 80 { 19 } else { 20 });
            if yr > 99 {
                continue;
            }
            let year = century * 100 + yr;
            if !(1980..=2107).contains(&year)
                || !(1..=12).contains(&mo)
                || d == 0
                || d > 31
                || hour >= 24
                || min >= 60
                || sec >= 60
            {
                continue;
            }
            if let Some(t) = crate::vfs::time::unpack(
                (hour << 11) | (min << 5) | (sec / 2),
                ((year - 1980) << 9) | (mo << 5) | d,
            ) {
                return Duration::from_secs(t as u64 + u64::from(sec & 1));
            }
        }
        last = sample;
    }
    Duration::ZERO
}
