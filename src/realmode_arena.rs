//! The native process arena occupies PSP:0000 through paragraph 6000h.
use dosrs_realmode::{
    PROGRAM, byte, store,
    system::{sw, w},
};
const FIRST: u16 = PROGRAM - 1;
pub(super) const END: u16 = 0x5fff;
fn block(seg: u16, owner: u16, end: u16) {
    store(seg, 0, 77);
    sw(seg, 1, owner);
    sw(seg, 3, end - seg - 1);
}
pub fn begin() {
    block(FIRST, PROGRAM, END);
}
fn next(seg: u16) -> u16 {
    let size = w(seg, 3);
    if size >= END - seg || !matches!(byte(seg, 0), 77 | 90) {
        0
    } else {
        seg + 1 + size
    }
}
fn limit(mut seg: u16) -> u16 {
    loop {
        let n = next(seg);
        if n == 0 || n == END || w(n, 1) != 0 {
            return n;
        }
        seg = n;
    }
}
fn split(seg: u16, end: u16, sz: u16) -> u32 {
    let after = seg + 1 + sz;
    if after < end {
        block(after, 0, end)
    }
    block(seg, PROGRAM, after);
    if seg == FIRST {
        sw(PROGRAM, 2, after);
    }
    u32::from(seg + 1)
}
fn low(ah: u16, sz: u16, target: u16, largest: &mut u16) -> u32 {
    let fit = byte(dosrs_realmode::COMMAND, 273);
    let mut chosen = 0;
    let mut best = u16::MAX;
    let mut seg = FIRST;
    while seg < END {
        let end = next(seg);
        if end == 0 {
            return 0x10007;
        }
        let owner = w(seg, 1);
        let match_alloc = ah == 0x48 && owner == 0;
        if match_alloc || (ah != 0x48 && target == seg + 1 && owner == PROGRAM) {
            let room = limit(seg);
            if room == 0 {
                return 0x10007;
            }
            if ah == 0x49 {
                block(seg, 0, room);
                return 0;
            }
            let avail = room - seg - 1;
            if avail > *largest {
                *largest = avail
            }
            if sz <= avail {
                if !match_alloc {
                    split(seg, room, sz);
                    return 0;
                }
                if fit == 0 {
                    return split(seg, room, sz);
                }
                if fit == 2 || avail < best {
                    chosen = seg;
                    best = avail
                }
            }
            if !match_alloc {
                return 0x10008;
            }
            seg = room;
        } else {
            seg = end;
        }
    }
    if chosen != 0 {
        let end = limit(chosen);
        if fit == 2 && end - chosen - 1 > sz {
            let high = end - sz - 1;
            block(chosen, 0, high);
            chosen = high
        }
        return split(chosen, end, sz);
    }
    if ah == 0x48 { 0x10008 } else { 0x10009 }
}

unsafe extern "Rust" {
    fn dos_upper_request(ax: u16, bx: u16, seg: u16, phase: u16) -> u32;
}
pub fn init() {
    store(END, 0, 90);
    sw(END, 1, 8);
    sw(END, 3, 0);
    store(dosrs_realmode::COMMAND, 273, 0);
    unsafe {
        dos_upper_request(0x5803, byte(dosrs_realmode::COMMAND, 272), 0, 0);
    }
}
pub fn release() {
    unsafe {
        dos_upper_request(0xf000, 0, 0, 0);
    }
}
pub fn request(ax: u16, size: u16, target: u16, room: &mut u16) -> u32 {
    let mut phase = 0;
    loop {
        let r = unsafe { dos_upper_request(ax, size, target, phase) };
        let code = r as u16;
        if code == 1 {
            if ax == 0x5801 {
                store(dosrs_realmode::COMMAND, 273, size & 3);
            }
            return r >> 16;
        }
        if code != 0 {
            if (r >> 16) as u16 > *room {
                *room = (r >> 16) as u16;
            }
            return 0x10000 | u32::from(code & 255);
        }
        if ax >> 8 == 0x58 {
            return match ax {
                0x5800 => u32::from(byte(dosrs_realmode::COMMAND, 273)),
                0x5801 if size < 3 => {
                    store(dosrs_realmode::COMMAND, 273, size);
                    0
                }
                0x5802 => 0,
                _ => 0x10001,
            };
        }
        if phase != 0 {
            return 0x10008;
        }
        let result = low(ax >> 8, size, target, room);
        if ax != 0x4800 || result != 0x10008 {
            return result;
        }
        phase = 1;
    }
}
