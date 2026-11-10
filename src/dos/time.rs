//! Date and time from `DOS/TIME.ASM`.
//!
//! Years run from 1980 through 2099. A year divisible by four is a leap year,
//! so February has 29 days. The day count starts at Tuesday. Hours, minutes,
//! seconds, and hundredths are rejected when they reach 24, 60, 60, and 100.
//! `DATE16` packs seven year bits, four month bits, and five day bits, and
//! packs hours, minutes, and seconds divided by two.

const FOUR_YEARS: u16 = 3 * 365 + 366;
const YEAR_DAYS: [u8; 8] = [200, 166, 200, 165, 200, 165, 200, 165];
const MONTH_DAYS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

#[derive(Clone, Copy)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub weekday: u8,
    pub day_count: u16,
    pub february: u8,
}

/// `$SET_DATE`. `AL` is 0 on success and -1 when the date is rejected.
pub fn set_date(year: u16, month: u8, day: u8) -> Result<Date, u8> {
    if year < 1980 {
        return Err(0xff);
    }
    let offset = year - 1980;
    if offset > 119 {
        return Err(0xff);
    }
    if month == 0 {
        return Err(0xff);
    }
    if day == 0 {
        return Err(0xff);
    }
    if month > 12 {
        return Err(0xff);
    }
    let months = month_lengths(offset as u8);
    if months[(month - 1) as usize] < day {
        return Err(0xff);
    }
    let day_count = count_days(offset as u8, month, day, &months);
    Ok(Date {
        year,
        month,
        day,
        weekday: weekday(day_count),
        day_count,
        february: months[1],
    })
}

/// `$SET_TIME`. Each field is rejected when it is greater than or equal to its limit.
pub fn set_time(hour: u8, minute: u8, second: u8, hundredths: u8) -> Result<(), u8> {
    if hour >= 24 {
        return Err(0xff);
    }
    if minute >= 60 {
        return Err(0xff);
    }
    if second >= 60 {
        return Err(0xff);
    }
    if hundredths >= 100 {
        return Err(0xff);
    }
    Ok(())
}

/// `READTIME` date conversion. A day count past 120 years is ignored.
pub fn date_from_count(day_count: u16) -> Option<Date> {
    if day_count >= FOUR_YEARS * 30 {
        return None;
    }
    let quads = day_count / FOUR_YEARS;
    let mut remain = day_count % FOUR_YEARS;
    let mut half_years = quads << 3;
    let mut index = 0usize;
    loop {
        let span = u16::from(YEAR_DAYS[index]);
        index += 1;
        if remain < span {
            break;
        }
        remain -= span;
        half_years += 1;
    }
    let odd_half = half_years & 1 == 1;
    let offset = (half_years >> 1) as u8;
    if odd_half {
        remain += 200;
    }
    let months = month_lengths(offset);
    let mut month = 1u8;
    index = 0;
    loop {
        let span = u16::from(months[index]);
        index += 1;
        if remain < span {
            break;
        }
        remain -= span;
        month += 1;
    }
    Some(Date {
        year: 1980 + u16::from(offset),
        month,
        day: (remain as u8) + 1,
        weekday: weekday(day_count),
        day_count,
        february: months[1],
    })
}

/// Date half of `DATE16`.
pub fn pack_date(year_offset: u8, month: u8, day: u8) -> u16 {
    let mut packed = u16::from(year_offset) << 8 | u16::from(month);
    let shifted = (packed as u8) << 4;
    packed = (packed & 0xff00) | u16::from(shifted);
    (packed << 1) | u16::from(day)
}

/// Time half of `DATE16`. Seconds are stored in two-second units.
pub fn pack_time(hour: u8, minute: u8, second: u8) -> u16 {
    let mut minutes = minute << 1;
    minutes <<= 1;
    let mut packed = u16::from(hour) << 8 | u16::from(minutes);
    packed <<= 1;
    packed <<= 1;
    packed <<= 1;
    packed | u16::from(second >> 1)
}

pub fn show() {
    let epoch = set_date(1980, 1, 1);
    let early = set_date(1979, 1, 1);
    let late = set_date(2100, 1, 1);
    let zero_month = set_date(1980, 0, 1);
    let zero_day = set_date(1980, 1, 0);
    let leap = set_date(1980, 2, 29);
    let no_leap = set_date(1981, 2, 29);
    let noon = set_time(23, 59, 59, 99);
    let bad_hour = set_time(24, 0, 0, 0);
    let bad_minute = set_time(0, 60, 0, 0);
    let bad_second = set_time(0, 0, 60, 0);
    let bad_hundredths = set_time(0, 0, 0, 100);
    let march = date_from_count(60);
    crate::bios::mscon::con_write_bytes(b"TIME ");
    put_date(epoch);
    crate::bios::mscon::con_write_bytes(b" EARLY");
    put_hex8(date_flag(early));
    crate::bios::mscon::con_write_bytes(b" LATE");
    put_hex8(date_flag(late));
    crate::bios::mscon::con_write_bytes(b" M");
    put_hex8(date_flag(zero_month));
    crate::bios::mscon::con_write_bytes(b" D");
    put_hex8(date_flag(zero_day));
    crate::bios::mscon::con_write_bytes(b" FEB");
    put_hex8(match leap {
        Ok(date) => date.february,
        Err(code) => code,
    });
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex16(match leap {
        Ok(date) => date.day_count,
        Err(_) => 0xffff,
    });
    crate::bios::mscon::con_write_bytes(b" N29");
    put_hex8(date_flag(no_leap));
    crate::bios::mscon::con_write_bytes(b" T");
    put_hex8(time_flag(noon));
    crate::bios::mscon::con_write_bytes(b" HR");
    put_hex8(time_flag(bad_hour));
    crate::bios::mscon::con_write_bytes(b" MN");
    put_hex8(time_flag(bad_minute));
    crate::bios::mscon::con_write_bytes(b" SC");
    put_hex8(time_flag(bad_second));
    crate::bios::mscon::con_write_bytes(b" HD");
    put_hex8(time_flag(bad_hundredths));
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex16(pack_date(0, 3, 1));
    crate::bios::mscon::con_write_bytes(b" K");
    put_hex16(pack_time(12, 30, 40));
    crate::bios::mscon::con_write_bytes(b" R");
    match march {
        Some(date) => {
            put_hex16(date.year);
            put_hex8(date.month);
            put_hex8(date.day);
        }
        None => put_hex16(0xffff),
    }
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_date(result: Result<Date, u8>) {
    match result {
        Ok(date) => {
            crate::bios::mscon::con_write_bytes(b"Y");
            put_hex16(date.year);
            crate::bios::mscon::con_write_bytes(b" M");
            put_hex8(date.month);
            crate::bios::mscon::con_write_bytes(b" D");
            put_hex8(date.day);
            crate::bios::mscon::con_write_bytes(b" W");
            put_hex8(date.weekday);
            crate::bios::mscon::con_write_bytes(b" C");
            put_hex16(date.day_count);
        }
        Err(code) => {
            crate::bios::mscon::con_write_bytes(b"ERR");
            put_hex8(code);
        }
    }
}

fn month_lengths(offset: u8) -> [u8; 12] {
    let mut months = MONTH_DAYS;
    months[1] = if offset & 3 == 0 { 29 } else { 28 };
    months
}

fn count_days(offset: u8, month: u8, day: u8, months: &[u8; 12]) -> u16 {
    let mut total = (u16::from(offset) >> 2) * FOUR_YEARS;
    let mut index = 0usize;
    let mut entries = u16::from(offset & 3) << 1;
    while entries > 0 {
        total = total.wrapping_add(u16::from(YEAR_DAYS[index]));
        index += 1;
        entries -= 1;
    }
    index = 0;
    let mut left = month - 1;
    while left > 0 {
        total = total.wrapping_add(u16::from(months[index]));
        index += 1;
        left -= 1;
    }
    total + u16::from(day - 1)
}

fn weekday(day_count: u16) -> u8 {
    ((day_count.wrapping_add(2)) % 7) as u8
}

fn date_flag(result: Result<Date, u8>) -> u8 {
    match result {
        Ok(_) => 0,
        Err(code) => code,
    }
}

fn time_flag(result: Result<(), u8>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
