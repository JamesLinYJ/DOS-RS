//! Read-only MBR planning. Unrelated partitions keep their slot, boot flag and bootstrap.
use crate::capacity;
use dosrs_realmode::{
    self as pc, byte, hd, raw_hd, store,
    system::{dw, sd, sw, w},
};
const TABLE: u16 = 0x3540;
const SECTOR: u16 = 0x3580;
pub const TAG: u32 = 0x31535244;
const PREP: u32 = 0x50535244;
pub const MIN: u32 = 8193;
pub const NEW: u16 = 1;
pub const PREPARED: u16 = 2;
pub const INSTALLED: u16 = 4;
pub const FOREIGN: u16 = 3;
pub const SPACE: u16 = 5;
pub const GPT: u16 = 6;
pub const INVALID: u16 = 7;
pub const SMALL: u16 = 8;
pub const EXTENDED: u16 = 9;
pub const FULL: u16 = 10;

#[derive(Clone, Copy)]
pub struct Target {
    pub state: u16,
    pub first: u32,
    pub size: u32,
    pub slot: u16,
    pub shared: bool,
}
impl Target {
    fn unavailable(state: u16) -> Self {
        Self {
            state,
            first: 0,
            size: 0,
            slot: 0,
            shared: false,
        }
    }
    pub fn same(&self, b: &Target) -> bool {
        self.state == b.state
            && self.first == b.first
            && self.size == b.size
            && self.slot == b.slot
            && self.shared == b.shared
    }
}
unsafe extern "Rust" {
    fn blank_edd(first: u32, count: u16, unit: u16) -> u16;
    fn zero_sector(segment: u16) -> u16;
}
fn empty(seg: u16) -> bool {
    unsafe { zero_sector(seg) != 0 }
}
fn pt(slot: u16) -> u16 {
    446 + 16 * slot
}
fn aligned(start: u32) -> u32 {
    if start > 0xfffff800 {
        u32::MAX
    } else {
        (start + 2047) & !2047
    }
}
pub fn inspect() -> Target {
    let cap = capacity();
    if cap < 2048 + MIN {
        return Target::unavailable(SMALL);
    }
    if !hd(0, TABLE, false) {
        return Target::unavailable(0);
    }
    if empty(TABLE) {
        if !hd(cap - 1, SECTOR, false) {
            return Target::unavailable(0);
        }
        if !empty(SECTOR) {
            return Target::unavailable(FOREIGN);
        }
        return Target {
            state: NEW,
            first: 2048,
            size: cap - 2048,
            slot: 0,
            shared: false,
        };
    }
    if w(TABLE, 510) != 0xaa55 || (w(TABLE, 11) == 512 && matches!(byte(TABLE, 0), 0xeb | 0xe9)) {
        return Target::unavailable(FOREIGN);
    }
    let mut free = 4;
    let mut used = 0;
    let mut active = 0;
    let mut owned = Target::unavailable(0);
    let mut i = 0;
    while i < 4 {
        let p = pt(i);
        let kind = byte(TABLE, p + 4);
        let first = dw(TABLE, p + 8);
        let size = dw(TABLE, p + 12);
        if kind == 0 {
            let mut j = 0;
            while j < 16 {
                if byte(TABLE, p + j) != 0 {
                    return Target::unavailable(INVALID);
                }
                j += 1;
            }
            if free == 4 {
                free = i
            }
            i += 1;
            continue;
        }
        if kind == 238 {
            return Target::unavailable(GPT);
        }
        if matches!(kind, 5 | 15 | 133) {
            return Target::unavailable(EXTENDED);
        }
        if first == 0 || first >= cap || size == 0 || size > cap - first {
            return Target::unavailable(INVALID);
        }
        let flag = byte(TABLE, p);
        if flag == 128 {
            active += 1
        } else if flag != 0 {
            return Target::unavailable(INVALID);
        }
        if active > 1 {
            return Target::unavailable(INVALID);
        }
        let mut j = 0;
        while j < i {
            let q = pt(j);
            if byte(TABLE, q + 4) != 0
                && first < dw(TABLE, q + 8) + dw(TABLE, q + 12)
                && dw(TABLE, q + 8) < first + size
            {
                return Target::unavailable(INVALID);
            }
            j += 1;
        }
        used += 1;
        if matches!(kind, 12 | 14) && size >= MIN {
            if !hd(first, SECTOR, false) {
                return Target::unavailable(0);
            }
            let state = if dw(SECTOR, 0) == PREP && dw(SECTOR, 4) == first && dw(SECTOR, 8) == size
            {
                PREPARED
            } else if w(SECTOR, 510) == 0xaa55
                && w(SECTOR, 11) == 512
                && dw(SECTOR, 28) == first
                && ((kind == 12 && dw(SECTOR, 67) == TAG) || (kind == 14 && dw(SECTOR, 39) == TAG))
            {
                INSTALLED
            } else if dw(TABLE, 432) == TAG
                && i == 0
                && first == 2048
                && size == cap - first
                && empty(SECTOR)
            {
                PREPARED
            } else {
                0
            };
            if state != 0 {
                if owned.state != 0 {
                    return Target::unavailable(INVALID);
                }
                owned = Target {
                    state,
                    first,
                    size,
                    slot: i,
                    shared: false,
                }
            }
        }
        i += 1;
    }
    if owned.state != 0 {
        owned.shared = used > 1 || dw(TABLE, 432) != TAG;
        return owned;
    }
    if free == 4 {
        return Target::unavailable(FULL);
    }
    if used == 0 {
        return Target::unavailable(FOREIGN);
    }
    // Select the largest 1 MiB-aligned gap; table entries need not be in physical order.
    let mut start = 2048;
    let mut best = 0;
    let mut best_at = 0;
    loop {
        let mut stop = cap;
        let mut after = cap;
        let mut next = 0;
        i = 0;
        while i < 4 {
            let p = pt(i);
            if byte(TABLE, p + 4) != 0 {
                let a = dw(TABLE, p + 8);
                let b = a + dw(TABLE, p + 12);
                if a <= start && b > start && b > next {
                    next = b
                }
                if a > start && a < stop {
                    stop = a;
                    after = b
                }
            }
            i += 1;
        }
        if next > start {
            start = aligned(next);
            if start >= cap {
                break;
            }
            continue;
        }
        if stop > start && stop - start > best {
            best = stop - start;
            best_at = start
        }
        if stop == cap {
            break;
        }
        start = aligned(after);
        if start >= cap {
            break;
        }
    }
    if best < MIN {
        return Target::unavailable(SMALL);
    }
    Target {
        state: SPACE,
        first: best_at,
        size: best,
        slot: free,
        shared: true,
    }
}
pub fn is_blank(first: u32, count: u32, mut bulk: bool) -> bool {
    let mut done = 0;
    let unit = pc::hard_unit();
    while done < count {
        let n = if count - done < 64 {
            (count - done) as u16
        } else {
            64
        };
        if bulk {
            match unsafe { blank_edd(first + done, n, unit) } {
                0 => return false,
                1 => {
                    done += n as u32;
                    continue;
                }
                _ => bulk = false,
            }
        }
        if !raw_hd(unit, first + done, SECTOR, false) || !empty(SECTOR) {
            return false;
        }
        done += 1;
    }
    true
}
pub fn publish(t: &Target, mbr: u16) -> bool {
    // Caller has checked the full extent. Publish a recognizable preparation record before the MBR.
    let mut p = 0;
    while p < 512 {
        store(SECTOR, p, 0);
        p += 1
    }
    sd(SECTOR, 0, PREP);
    sd(SECTOR, 4, t.first);
    sd(SECTOR, 8, t.size);
    if !hd(t.first, SECTOR, true) {
        return false;
    }
    if !t.shared {
        pc::copy_bytes(mbr, 0, TABLE, 0, 512);
        sd(TABLE, 432, TAG)
    }
    let q = pt(t.slot);
    store(TABLE, q, if t.shared { 0 } else { 128 });
    store(TABLE, q + 4, if t.size >= 70000 { 12 } else { 14 });
    for_chs(q + 1);
    for_chs(q + 5);
    sd(TABLE, q + 8, t.first);
    sd(TABLE, q + 12, t.size);
    sw(TABLE, 510, 0xaa55);
    hd(0, TABLE, true)
}
fn for_chs(p: u16) {
    store(TABLE, p, 254);
    store(TABLE, p + 1, 255);
    store(TABLE, p + 2, 255);
}
