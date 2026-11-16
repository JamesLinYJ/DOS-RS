//! Initialize flat DOS character drivers above the disk cache, before starting COMMAND.COM.
use dosrs_realmode::{
    self as pc,
    system::{Media, sd, sw, w},
};
const REQUEST: u16 = 0x34a0;
const CONFIG: u16 = 0x3400;
unsafe extern "Rust" {
    fn device_call(segment: u16, offset: u16, packet: u16, at: u16);
}

pub struct Devices {
    next: u16,
    last: u16,
}
impl Devices {
    pub fn new() -> Self {
        Self {
            next: 0x9000,
            last: 0,
        }
    }
    pub fn load(&mut self, media: &Media, mut start: u16, end: u16) -> bool {
        while start < end && pc::byte(CONFIG, start) == 32 {
            start += 1;
        }
        let mut at = start;
        if at + 1 < end && pc::byte(CONFIG, at + 1) == 58 {
            let letter = pc::byte(CONFIG, at) | 32;
            let boot = pc::boot_unit();
            if letter != if boot >= 128 { 99 } else { 97 + boot } {
                return false;
            }
            at += 2;
        }
        if at < end && pc::byte(CONFIG, at) == 92 {
            at += 1;
        }
        let mut stop = at;
        while stop < end && pc::byte(CONFIG, stop) > 32 {
            stop += 1;
        }
        if !pc::filename(CONFIG, at, stop - at) {
            return false;
        }
        let mut name = [0u8; 11];
        let mut i = 0;
        while i < 11 {
            name[i as usize] = pc::byte(pc::COMMAND, 128 + i) as u8;
            i += 1;
        }
        let Some(file) = media.lookup(&name) else {
            return false;
        };
        let seg = self.next;
        let kib = w(0x40, 0x13);
        let limit = if kib > 640 { 640 * 64 } else { kib * 64 };
        if seg >= limit
            || file.size < 18
            || file.size > 0xfff0
            || file.size > (limit - seg) as u32 * 16
        {
            return false;
        }
        if !media.fetch(&file, 0, file.size as u16, seg, 0)
            || w(seg, 0) == 0x5a4d
            || w(seg, 4) & 0x8000 == 0
            || w(seg, 6) as u32 >= file.size
            || w(seg, 8) as u32 >= file.size
        {
            return false;
        }
        i = 0;
        while i < 24 {
            pc::store(REQUEST, i, 0);
            i += 1;
        }
        pc::store(REQUEST, 0, 23);
        sw(REQUEST, 16, limit);
        // INIT command line pointer. Terminate the copy with a DOS CR.
        i = 0;
        while start + i < end && i < 126 {
            pc::store(REQUEST, 32 + i, pc::byte(CONFIG, start + i));
            i += 1;
        }
        pc::store(REQUEST, 32 + i, 13);
        sw(REQUEST, 18, 32);
        sw(REQUEST, 20, REQUEST);
        // SAFETY: checked entry offsets refer to the resident image; ES:BX is the INIT packet.
        unsafe {
            device_call(seg, w(seg, 6), REQUEST, 0);
            device_call(seg, w(seg, 8), REQUEST, 0);
        }
        if w(REQUEST, 3) & 0x8100 != 0x100 {
            return false;
        }
        let top = (w(REQUEST, 16) as u32) * 16 + w(REQUEST, 14) as u32;
        // Drivers can return success with zero resident bytes (e.g. on a 286).
        if top == seg as u32 * 16 {
            return true;
        }
        if top < seg as u32 * 16 || top > limit as u32 * 16 {
            return false;
        }
        sd(seg, 0, u32::MAX);
        if self.last != 0 {
            sw(self.last, 0, 0);
            sw(self.last, 2, seg);
        }
        self.last = seg;
        self.next = (((top + 4095) >> 12) << 8) as u16;
        true
    }
}
