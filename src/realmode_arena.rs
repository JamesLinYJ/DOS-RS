//! Conventional memory ends just below the resident font workspace.
use dosrs_realmode::{
    PROGRAM, byte, store,
    system::{sw, w},
};
const FIRST: u16 = PROGRAM - 1;
pub(super) fn end() -> u16 {
    dosrs_realmode::font_workspace() - 1
}
fn block(seg: u16, owner: u16, end: u16) {
    store(seg, 0, 77);
    sw(seg, 1, owner);
    sw(seg, 3, end - seg - 1);
}
pub fn current() -> u16 {
    w(dosrs_realmode::COMMAND, dosrs_realmode::layout::CURRENT_PSP)
}
pub fn select(psp: u16) {
    sw(
        dosrs_realmode::COMMAND,
        dosrs_realmode::layout::CURRENT_PSP,
        psp,
    );
    sw(
        dosrs_realmode::dos_tables::LIST_SEG,
        dosrs_realmode::dos_tables::SDA + 16,
        psp,
    );
}
pub fn begin() {
    select(PROGRAM);
    block(FIRST, PROGRAM, end());
}
fn next(seg: u16) -> u16 {
    let arena_end = end();
    let size = w(seg, 3);
    if size >= arena_end - seg || !matches!(byte(seg, 0), 77 | 90) {
        0
    } else {
        seg + 1 + size
    }
}
fn limit(mut seg: u16) -> u16 {
    let arena_end = end();
    loop {
        let n = next(seg);
        if n == 0 || n == arena_end || w(n, 1) != 0 {
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
    let owner = current();
    block(seg, owner, after);
    if seg + 1 == owner {
        sw(owner, 2, after);
    }
    u32::from(seg + 1)
}
fn low(ah: u16, sz: u16, target: u16, largest: &mut u16) -> u32 {
    let arena_end = end();
    let fit = byte(dosrs_realmode::COMMAND, 273);
    let mut chosen = 0;
    let mut best = u16::MAX;
    let mut seg = FIRST;
    while seg < arena_end {
        let end = next(seg);
        if end == 0 {
            return 0x10007;
        }
        let owner = w(seg, 1);
        let match_alloc = ah == 0x48 && owner == 0;
        if match_alloc || (ah != 0x48 && target == seg + 1 && owner == current()) {
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
    let arena_end = end();
    store(arena_end, 0, 90);
    sw(arena_end, 1, 8);
    sw(arena_end, 3, 0);
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

/// Allocate the largest contiguous conventional block for a child process.
pub fn child_block() -> Option<(u16, u16)> {
    let mut largest = 0;
    low(0x48, 0xffff, 0, &mut largest);
    if largest < 16 {
        return None;
    }
    let child = low(0x48, largest, 0, &mut largest);
    if child > 65535 {
        return None;
    }
    let psp = child as u16;
    sw(psp - 1, 1, psp);
    Some((psp, largest))
}
pub fn discard(psp: u16) {
    unsafe {
        dos_upper_request(0xf000, 0, psp, 0);
    }
    let mut m = FIRST;
    while m < end() {
        let n = next(m);
        if n == 0 {
            break;
        }
        if w(m, 1) == psp {
            sw(m, 1, 0);
        }
        m = n;
    }
}

/// Claim either the highest suitable conventional range or an exact previous
/// address. A suspended interpreter must resume at its original CS:IP.
pub(super) fn code_block(paragraphs: u16, previous: u16) -> Option<u16> {
    let mut m = FIRST;
    let mut chosen = 0;
    let mut base = 0;
    let mut bound = 0;
    while m < end() {
        let next_block = next(m);
        if next_block == 0 {
            return None;
        }
        if w(m, 1) != 0 {
            m = next_block;
            continue;
        }
        let room = limit(m);
        if room == 0 {
            return None;
        }
        if room - m > paragraphs {
            let at = if previous == 0 {
                room - paragraphs
            } else {
                previous
            };
            if at > m && at <= room - paragraphs {
                chosen = m;
                base = at;
                bound = room;
            }
        }
        m = room;
    }
    if base == 0 {
        return None;
    }
    if base - 1 != chosen {
        block(chosen, 0, base - 1);
    }
    split(base - 1, bound, paragraphs);
    Some(base)
}

/// Try a process-owned upper-memory allocation without changing DOS allocation
/// policy for the calling program. Only linked UMBs are eligible.
pub(super) fn upper_block(paragraphs: u16) -> Option<u16> {
    let mut room = 0;
    if request(0x5802, 0, 0, &mut room) != 1 {
        return None;
    }
    let previous = request(0x5800, 0, 0, &mut room);
    if previous > 255 || request(0x5801, 0x40 | (previous as u16 & 3), 0, &mut room) != 0 {
        return None;
    }
    let block = request(0x4800, paragraphs, 0, &mut room);
    request(0x5801, previous as u16, 0, &mut room);
    if block > u32::from(u16::MAX) {
        None
    } else {
        Some(block as u16)
    }
}
