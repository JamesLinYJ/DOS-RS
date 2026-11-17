//! Native INT 21h character device dispatch through installed DOS driver headers.
use super::devices::{REQUEST, device_call};
use dosrs_realmode::{
    self as pc, byte,
    dos_tables::{LIST, LIST_SEG, SEG, entry, handle, set_handle},
    store,
    system::{dw, sd, sw, w},
};

const NUL: u32 = ((LIST_SEG as u32) << 16) | (LIST as u32 + 0x22);
const REGISTRY: u16 = 0xa00;
const ERROR: u32 = 0x10000;
const UNHANDLED: u32 = u32::MAX;

pub fn reset() {
    sw(SEG, REGISTRY, 0);
}

pub fn register(seg: u16, top: u32) -> bool {
    let n = w(SEG, REGISTRY);
    let size = top - (u32::from(seg) << 4);
    if n >= 16 || size < 18 || u32::from(w(seg, 6)) >= size || u32::from(w(seg, 8)) >= size {
        return false;
    }
    let at = REGISTRY + 2 + 4 * n;
    sw(SEG, at, seg);
    sw(SEG, at + 2, if size > 65535 { 65535 } else { size as u16 });
    sw(SEG, REGISTRY, n + 1);
    true
}

// Only invoke entries belonging to resident images that passed INIT. Bounds
// are retained independently of the DOS-visible linked list and SFT.
fn valid(header: u32) -> bool {
    if header == NUL || header == pc::dos_tables::CON {
        return true;
    }
    let seg = (header >> 16) as u16;
    let off = header as u16;
    let mut i = 0;
    let count = w(SEG, REGISTRY);
    while i < count && i < 16 {
        let at = REGISTRY + 2 + 4 * i;
        if w(SEG, at) == seg {
            let end = w(SEG, at + 2);
            return end >= 18
                && off <= end - 18
                && w(seg, off + 4) & 0x8000 != 0
                && w(seg, off + 6) < end
                && w(seg, off + 8) < end;
        }
        i += 1;
    }
    false
}

fn upper(ch: u16) -> u16 {
    if ch >= 97 && ch <= 122 { ch - 32 } else { ch }
}

fn lookup(seg: u16, at: u16) -> u32 {
    let mut start = at;
    let mut pos = at;
    let mut scanned = 0u16;
    while scanned < 128 {
        let ch = byte(seg, pos);
        if ch == 0 {
            let mut len = 0;
            while start + len < pos && byte(seg, start + len) != 46 && byte(seg, start + len) != 58
            {
                len += 1;
            }
            if len == 0 || len > 8 || (byte(seg, start + len) == 58 && start + len != pos - 1) {
                return 0;
            }
            let mut p = NUL;
            let mut visited = 0u16;
            while visited < 17 {
                if !valid(p) {
                    break;
                }
                let s = (p >> 16) as u16;
                let off = p as u16;
                let mut i = 0;
                while i < 8 {
                    let c = if i < len {
                        upper(byte(seg, start + i))
                    } else {
                        32
                    };
                    if upper(byte(s, off + 10 + i)) != c {
                        break;
                    }
                    i += 1;
                }
                if i == 8 {
                    return p;
                }
                p = dw(s, off);
                visited += 1;
            }
            return 0;
        }
        if pos == 65535 {
            break;
        }
        if ch == 92 || ch == 47 || (ch == 58 && pos == at + 1) {
            start = pos + 1;
        }
        pos += 1;
        scanned += 1;
    }
    0
}

fn request(header: u32, command: u16, seg: u16, off: u16, count: u16) -> u32 {
    if !valid(header) {
        return ERROR + 6;
    }
    if header == NUL {
        return match command {
            4 => 0,
            8 => u32::from(count),
            6 | 10 => 255,
            13 | 14 => 0,
            _ => ERROR + 1,
        };
    }
    let driver = (header >> 16) as u16;
    let at = header as u16;
    let mut i = 0;
    while i < 22 {
        store(REQUEST, i, 0);
        i += 1;
    }
    store(REQUEST, 0, 22);
    store(REQUEST, 2, command);
    sw(REQUEST, 14, off);
    sw(REQUEST, 16, seg);
    sw(REQUEST, 18, count);
    // Both routine offsets were checked against the same resident image.
    unsafe {
        device_call(driver, w(driver, at + 6), REQUEST, 0);
        device_call(driver, w(driver, at + 8), REQUEST, 0);
    }
    let status = w(REQUEST, 3);
    if status & 0x8000 != 0 {
        return ERROR + 0x13 + u32::from(status & 255);
    }
    if status & 0x100 == 0 {
        return ERROR + 31;
    }
    if command == 6 || command == 10 {
        return if status & 0x200 == 0 { 255 } else { 0 };
    }
    let done = w(REQUEST, 18);
    if done > count {
        ERROR + 31
    } else {
        u32::from(done)
    }
}

fn attribute(header: u32) -> u16 {
    w((header >> 16) as u16, header as u16 + 4)
}

pub fn open_request(seg: u16, off: u16) -> u16 {
    let h = (u32::from(seg) << 16) | u32::from(off);
    request(h, 13, 0, 0, 0) as u16
}

fn release(psp: u16, fd: u16, s: u16) -> u32 {
    let refs = w(SEG, s);
    let h = dw(SEG, s + 7);
    if !valid(h) {
        return ERROR + 6;
    }
    if refs == 1 && attribute(h) & 0x0800 != 0 {
        let rc = request(h, 14, 0, 0, 0);
        if rc >= ERROR {
            return rc;
        }
    }
    set_handle(psp, fd, 255);
    sw(SEG, s, refs - 1);
    0
}

pub fn close(psp: u16, fd: u16) -> u16 {
    let sys = handle(psp, fd);
    if sys < 5 || sys >= pc::dos_tables::COUNT {
        return 0;
    }
    let s = entry(sys);
    if w(SEG, s) == 0 || w(SEG, s + 5) & 0x80 == 0 {
        return 0;
    }
    if release(psp, fd, s) == 0 { 1 } else { 2 }
}

fn service(ss: u16, sp: u16, psp: u16) -> u32 {
    let ax = w(ss, sp);
    let fd = w(ss, sp + 2);
    let ds = w(ss, sp + 16);
    let off = w(ss, sp + 6);
    if ax >> 8 == 0x3d || ax >> 8 == 0x3c {
        if ds == 0 && off == 0 {
            return ERROR + 5;
        }
        let h = lookup(ds, off);
        if h == 0 {
            return UNHANDLED;
        }
        sd(REQUEST, 32, h);
        return UNHANDLED - 1;
    }
    let ioctl = ax >> 8 == 0x44;
    if ioctl && !matches!(ax, 0x4400 | 0x4402 | 0x4403 | 0x4406 | 0x4407) {
        return UNHANDLED;
    }
    let sys = handle(psp, fd);
    if sys >= pc::dos_tables::COUNT || w(SEG, entry(sys)) == 0 {
        return if ioctl { ERROR + 6 } else { UNHANDLED };
    }
    let s = entry(sys);
    let flags = w(SEG, s + 5);
    let device = flags & 0x80 != 0;
    let h = dw(SEG, s + 7);
    if ax == 0x4400 {
        let high = if device && sys >= 5 {
            if !valid(h) {
                return ERROR + 6;
            }
            attribute(h) & 0xff00
        } else if device {
            0x8000
        } else {
            0
        };
        let value = high | (flags & 255);
        sw(ss, sp + 6, value);
        return u32::from(value);
    }
    if !device || (sys < 5 && h != pc::dos_tables::CON) {
        if ax == 0x4407 {
            return if device { 255 } else { 0 };
        }
        return if ioctl { ERROR + 1 } else { UNHANDLED };
    }
    if sys < 5 && (ax >> 8 == 0x3e || (ax >> 8 == 0x40 && (sys == 1 || sys == 2))) {
        return UNHANDLED;
    }
    if !valid(h) {
        return ERROR + 6;
    }
    let command = match ax {
        0x3e00..=0x3eff => return release(psp, fd, s),
        0x3f00..=0x3fff => 4,
        0x4000..=0x40ff => 8,
        0x4200..=0x4202 => {
            sw(ss, sp + 6, 0);
            return 0;
        }
        0x4402 | 0x4403 if attribute(h) & 0x4000 != 0 => {
            if ax == 0x4402 {
                3
            } else {
                12
            }
        }
        0x4406 | 0x4407 => return request(h, if ax == 0x4406 { 6 } else { 10 }, 0, 0, 0),
        _ => return ERROR + 1,
    };
    let access = w(SEG, s + 2) & 7;
    if (command == 8 && access == 0) || (command == 4 && access == 1) {
        return ERROR + 5;
    }
    let count = w(ss, sp + 4);
    request(h, command, ds, off, count)
}

pub fn dispatch(ss: u16, sp: u16, psp: u16) -> u16 {
    let result = service(ss, sp, psp);
    if result == UNHANDLED {
        return 0;
    }
    if result == UNHANDLED - 1 {
        return 2;
    }
    sw(ss, sp, result as u16);
    let flags = w(ss, sp + 22) & !1;
    sw(ss, sp + 22, flags | (result >= ERROR) as u16);
    1
}
