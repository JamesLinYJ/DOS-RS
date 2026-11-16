//! Conversion between UTC Unix time and the timezone-free DOS packed fields.
const UNIX_TO_1980: i64 = 315532800;
fn days(year: i64, month: u16) -> u16 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
pub(crate) fn unpack(time: u16, date: u16) -> Option<i64> {
    let year = 1980 + i64::from(date >> 9);
    let month = (date >> 5) & 15;
    let day = date & 31;
    let hour = time >> 11;
    let minute = time >> 5 & 63;
    let sec = (time & 31) * 2;
    if !(1..=12).contains(&month)
        || day == 0
        || day > days(year, month)
        || hour >= 24
        || minute >= 60
        || sec >= 60
    {
        return None;
    }
    let mut elapsed = 0i64;
    for y in 1980..year {
        elapsed += 337 + i64::from(days(y, 2));
    }
    for m in 1..month {
        elapsed += i64::from(days(year, m));
    }
    Some(
        UNIX_TO_1980
            + (elapsed + i64::from(day - 1)) * 86400
            + i64::from(hour) * 3600
            + i64::from(minute) * 60
            + i64::from(sec),
    )
}
pub(super) fn pack(secs: i64) -> (u16, u16) {
    let delta = secs.saturating_sub(UNIX_TO_1980).max(0);
    let mut nd = delta / 86400;
    let mut year = 1980;
    while year <= 2107 {
        let n = 337 + i64::from(days(year, 2));
        if nd < n {
            break;
        }
        nd -= n;
        year += 1;
    }
    if year > 2107 {
        return (0xbf7d, 0xff9f);
    }
    let mut month = 1;
    while nd >= i64::from(days(year, month)) {
        nd -= i64::from(days(year, month));
        month += 1;
    }
    let tod = delta % 86400;
    (
        ((tod / 3600) as u16) << 11 | ((tod / 60 % 60) as u16) << 5 | (tod % 60 / 2) as u16,
        (((year - 1980) as u16) << 9) | (month << 5) | (nd as u16 + 1),
    )
}
