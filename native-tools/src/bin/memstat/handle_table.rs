//! Validate the public XMS table through the same far pointer across mutations.
use super::{MULTIPLEX, Regs, XMS, byte, call, ems, w, xms};

fn free_kib(seg: u16, header: u16) -> u32 {
    let rows = w(seg, header + 2);
    let first = w(seg, header + 4);
    if rows == 0 || u32::from(first) + u32::from(rows) * 10 > 65536 {
        return u32::MAX;
    }
    let mut total = 0u32;
    for n in 0..rows {
        let row = first + n * 10;
        if byte(seg, row) == 1 {
            let kib = u32::from(w(seg, row + 6)) | u32::from(w(seg, row + 8)) << 16;
            if kib > u32::MAX - total {
                return u32::MAX;
            }
            total += kib;
        }
    }
    total
}

pub(super) fn test(r: &mut Regs) -> bool {
    call(r, MULTIPLEX, 0x4309, 0, 0);
    let (seg, header) = (r[6], r[1]);
    if r[0] & 255 != 0x43
        || header > 0xfff8
        || w(seg, header) != 0x0a01
        || w(seg, header + 6) != seg
    {
        return false;
    }
    call(r, XMS, 0x0800, 0, 0);
    let before = u32::from(r[3]);
    if free_kib(seg, header) != before || !xms(r, 0x0900, 0) {
        return false;
    }
    let handle = r[3];
    let first = w(seg, header + 4);
    let valid = handle >= first
        && handle < 0xfff7
        && (handle - first) % 10 == 0
        && (handle - first) / 10 < w(seg, header + 2);
    let mut ok = valid && w(seg, handle) == 2 && w(seg, handle + 6) == 0;
    call(r, XMS, 0x0f00, 7, handle);
    ok = ok && r[0] == 1 && w(seg, handle + 6) == 7;
    if xms(r, 0x0c00, handle) {
        let physical = u32::from(r[3]) << 16 | u32::from(r[1]);
        ok = ok
            && w(seg, handle) == 0x0102
            && (u32::from(w(seg, handle + 2)) | u32::from(w(seg, handle + 4)) << 16)
                == physical >> 10;
        ok = xms(r, 0x0d00, handle) && ok && w(seg, handle) == 2;
    } else {
        ok = false;
    }
    if ems(r, 0x4300, 1, 0) {
        let em = r[3];
        ok = ok && before >= 23 && free_kib(seg, header) == before - 23;
        ok = ems(r, 0x4500, 0, em) && ok;
    } else {
        ok = false;
    }
    let released = xms(r, 0x0a00, handle);
    ok && released && w(seg, handle) == 4 && free_kib(seg, header) == before
}
