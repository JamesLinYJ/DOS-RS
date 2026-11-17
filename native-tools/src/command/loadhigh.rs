//! DOS 6 LOADHIGH uses the allocator itself to hold excluded UMBs while the child runs.
use super::{dos, parse};
use dosrs_realmode::{
    byte,
    system::{sw, w},
};

struct Regions {
    numbers: [u16; 63],
    minimum: [u16; 63],
    len: usize,
    shrink: bool,
}

fn number(s: &[u8], at: &mut usize) -> Option<u32> {
    let first = *at;
    let mut value = 0u32;
    while *at < s.len() && s[*at] >= b'0' && s[*at] <= b'9' {
        if value > 429496728 {
            return None;
        }
        value = value * 10 + u32::from(s[*at] - b'0');
        *at += 1;
    }
    if *at == first { None } else { Some(value) }
}

impl Regions {
    fn read(mut args: &[u8]) -> Option<(Self, &[u8])> {
        let mut list = Self {
            numbers: [0; 63],
            minimum: [0; 63],
            len: 0,
            shrink: false,
        };
        while !args.is_empty() && args[0] == b'/' {
            let (switch, rest) = parse::word(args);
            args = rest;
            if parse::equal(switch, b"/S") {
                list.shrink = true;
                continue;
            }
            if switch.len() < 4 || !parse::equal(parse::head(switch, 3), b"/L:") {
                return None;
            }
            let mut i = 3;
            loop {
                let region = number(switch, &mut i)?;
                if region > 65535 || list.len == list.numbers.len() {
                    return None;
                }
                let mut minimum = 0;
                if i < switch.len() && switch[i] == b',' {
                    i += 1;
                    let bytes = number(switch, &mut i)?;
                    minimum = if bytes >= 1048560 {
                        u16::MAX
                    } else {
                        ((bytes + 15) >> 4) as u16
                    };
                }
                list.numbers[list.len] = region as u16;
                list.minimum[list.len] = minimum;
                list.len += 1;
                if i == switch.len() {
                    break;
                }
                if switch[i] != b';' {
                    return None;
                }
                i += 1;
            }
        }
        if args.is_empty() {
            None
        } else {
            Some((list, args))
        }
    }
    fn min(&self, region: u16) -> Option<u16> {
        let mut i = 0;
        while i < self.len {
            if self.numbers[i] == region {
                return Some(self.minimum[i]);
            }
            i += 1;
        }
        None
    }
}

fn memory(ax: u16, size: u16, seg: u16) -> Result<u16, u16> {
    let mut r = [0; 12];
    r[0] = ax;
    r[1] = size;
    r[6] = seg;
    if dos::call(&mut r) {
        Ok(r[0])
    } else {
        Err(r[1])
    }
}

fn largest() -> u16 {
    match memory(0x4800, u16::MAX, 0) {
        Err(size) => size,
        Ok(seg) => {
            let _ = memory(0x4900, 0, seg);
            0
        }
    }
}

// The first bridge spans conventional RAM to the option-ROM area. Subsequent
// DOS-owned control MCBs named "SC" span holes between published UMB regions.
fn region(mcb_of: u16) -> u16 {
    let mut r = [0; 12];
    r[0] = 0x5200;
    if !dos::call(&mut r) {
        return 0;
    }
    let mut seg = w(r[6], r[1].wrapping_sub(2));
    let mut n = 0;
    loop {
        let kind = byte(seg, 0);
        if kind != 77 && kind != 90 {
            break;
        }
        if seg == mcb_of || kind == 90 {
            break;
        }
        let size = w(seg, 3);
        if size >= u16::MAX - seg {
            break;
        }
        let next = seg + size + 1;
        if (n == 0 && seg < 0xa000 && next >= 0xa000)
            || (n != 0 && w(seg, 1) == 8 && w(seg, 8) == 0x4353)
        {
            n += 1;
        }
        seg = next;
    }
    n
}

// These are fully allocated, process-owned paragraphs. Their first words form
// temporary lists, avoiding a fixed limit on fragmented UMBs or kernel metadata writes.
struct Blocks(u16);
impl Blocks {
    fn push(&mut self, seg: u16) {
        sw(seg, 0, self.0);
        self.0 = seg;
    }
    fn pop(&mut self) -> Option<u16> {
        let seg = self.0;
        if seg == 0 {
            return None;
        }
        self.0 = w(seg, 0);
        Some(seg)
    }
    fn release(&mut self) {
        while let Some(seg) = self.pop() {
            let _ = memory(0x4900, 0, seg);
        }
    }
}

struct Hold {
    pending: Blocks,
    blocked: Blocks,
    available: Blocks,
    eligible: bool,
}
impl Hold {
    fn new() -> Self {
        Self {
            pending: Blocks(0),
            blocked: Blocks(0),
            available: Blocks(0),
            eligible: false,
        }
    }
    fn collect(&mut self, list: &Regions) -> bool {
        loop {
            let free = largest();
            if free == 0 {
                break;
            }
            let Ok(seg) = memory(0x4800, free, 0) else {
                return false;
            };
            self.pending.push(seg);
        }
        while let Some(seg) = self.pending.pop() {
            let size = w(seg - 1, 3);
            let wanted = list.min(region(seg - 1));
            match wanted {
                Some(n) if size >= n => {
                    self.available.push(seg);
                    if list.shrink && n != 0 && size - n > 1 {
                        if memory(0x4a00, n, seg).is_err() {
                            return false;
                        }
                        // All other UMBs are held, so only this block's tail can be reserved.
                        let tail = largest();
                        let Ok(rest) = memory(0x4800, tail, 0) else {
                            return false;
                        };
                        self.blocked.push(rest);
                    }
                    self.eligible = true;
                }
                _ => self.blocked.push(seg),
            }
        }
        self.available.release();
        true
    }
    fn finish(&mut self) {
        self.pending.release();
        self.blocked.release();
        self.available.release();
    }
}

pub fn run(sh: &mut super::Shell, f: &mut super::batch::Frame, args: &[u8], called: bool) {
    let Some((list, args)) = Regions::read(args) else {
        super::execute::syntax(sh);
        return;
    };
    let (name, tail) = parse::word(args);
    let strategy = dos::service(0x5800, 0, 0, 0).unwrap_or(0);
    let linked = dos::service(0x5802, 0, 0, 0).unwrap_or(0) & 255;
    let supports_umb = dos::service(0x5803, 1, 0, 0).is_ok();
    let mut held = Hold::new();
    let mut valid = true;
    if supports_umb && list.len > 0 {
        let last = region(u16::MAX);
        let mut i = 0;
        while i < list.len {
            if list.numbers[i] > last {
                valid = false;
                break;
            }
            i += 1;
        }
        if valid && last > 0 {
            valid = dos::service(0x5801, 0x40, 0, 0).is_ok() && held.collect(&list);
        }
    }
    if valid {
        let low = list.len > 0 && (list.numbers[0] == 0 || !held.eligible);
        if supports_umb && low {
            let _ = dos::service(0x5803, 0, 0, 0);
        }
        let mode = if low { 0 } else { 0x80 };
        let _ = dos::service(0x5801, mode, 0, 0);
        super::execute::external(sh, f, name, tail, called);
    } else {
        dos::err(b"Invalid memory region or insufficient upper memory\r\n");
        sh.status = 1;
    }
    if held.blocked.0 != 0 || held.pending.0 != 0 || held.available.0 != 0 {
        let _ = dos::service(0x5803, 1, 0, 0);
    }
    held.finish();
    let _ = dos::service(0x5801, strategy, 0, 0);
    let _ = dos::service(0x5803, linked, 0, 0);
}
