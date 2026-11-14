//! PC/AT interrupt controller, channel-zero interval timer and keyboard interface.
use alloc::collections::VecDeque;

#[derive(Debug)]
pub(super) struct At {
    vector: u8,
    mask: u8,
    requests: u8,
    service: u8,
    init: u8,
    icw4: bool,
    single: bool,
    auto_eoi: bool,
    read_service: bool,
    divisor: u32,
    ticks: u32,
    access: u8,
    timer_low: Option<u8>,
    count_latch: Option<u16>,
    read_high: bool,
    queue: VecDeque<u8>,
    command_byte: u8,
    command: u8,
    key_arg: bool,
}
impl At {
    pub fn new() -> Self {
        Self {
            vector: 8,
            mask: 0xfc,
            requests: 0,
            service: 0,
            init: 0,
            icw4: false,
            single: false,
            auto_eoi: false,
            read_service: false,
            divisor: 65536,
            ticks: 0,
            access: 3,
            timer_low: None,
            count_latch: None,
            read_high: false,
            queue: VecDeque::new(),
            command_byte: 0x41,
            command: 0,
            key_arg: false,
        }
    }
    pub fn key(&mut self, scan: u8) {
        if self.command_byte & 16 != 0 {
            return;
        }
        if self.command_byte & 64 != 0 || scan >= 0xe0 {
            self.queue.push_back(scan);
        } else {
            // AT set-1 make codes indexed to their set-2 equivalents.
            const SET2: &[u8] = &[
                0, 0x76, 0x16, 0x1e, 0x26, 0x25, 0x2e, 0x36, 0x3d, 0x3e, 0x46, 0x45, 0x4e, 0x55,
                0x66, 0x0d, 0x15, 0x1d, 0x24, 0x2d, 0x2c, 0x35, 0x3c, 0x43, 0x44, 0x4d, 0x54, 0x5b,
                0x5a, 0x14, 0x1c, 0x1b, 0x23, 0x2b, 0x34, 0x33, 0x3b, 0x42, 0x4b, 0x4c, 0x52, 0x0e,
                0x12, 0x5d, 0x1a, 0x22, 0x21, 0x2a, 0x32, 0x31, 0x3a, 0x41, 0x49, 0x4a, 0x59, 0x7c,
                0x11, 0x29, 0x58, 0x05, 0x06, 0x04, 0x0c, 0x03, 0x0b, 0x83, 0x0a, 0x01, 0x09, 0x77,
                0x7e, 0x6c, 0x75, 0x7d, 0x7b, 0x6b, 0x73, 0x74, 0x79, 0x69, 0x72, 0x7a, 0x70, 0x71,
                0, 0, 0x61, 0x78, 0x07,
            ];
            let Some(&code) = SET2.get(usize::from(scan & 127)) else {
                return;
            };
            if code == 0 {
                return;
            }
            if scan & 128 != 0 {
                self.queue.push_back(0xf0)
            }
            self.queue.push_back(code);
        }
        self.refresh_keyboard();
    }
    fn refresh_keyboard(&mut self) {
        if !self.queue.is_empty() && self.command_byte & 1 != 0 {
            self.requests |= 2;
        }
    }
    pub fn advance(&mut self, clocks: u32) {
        self.ticks += clocks;
        if self.ticks >= self.divisor {
            self.ticks %= self.divisor;
            self.requests |= 1;
        }
    }
    pub fn interrupt(&mut self) -> Option<u8> {
        let irq = (self.requests & !self.mask).trailing_zeros();
        if irq >= 8 || irq >= self.service.trailing_zeros() {
            return None;
        }
        self.requests &= !(1 << irq);
        if !self.auto_eoi {
            self.service |= 1 << irq;
        }
        Some(self.vector + irq as u8)
    }
    pub fn eoi(&mut self) {
        if self.service != 0 {
            self.service &= self.service - 1;
        }
    }
    pub fn port(&mut self, p: u16, write: bool, v: u8, a20: &mut bool) -> Option<u8> {
        if !write {
            return Some(match p {
                0x20 => {
                    if self.read_service {
                        self.service
                    } else {
                        self.requests
                    }
                }
                0x21 => self.mask,
                0x40 => {
                    let count = self
                        .count_latch
                        .unwrap_or((self.divisor - self.ticks) as u16);
                    let high = self.access == 2 || self.access == 3 && self.read_high;
                    self.read_high = self.access == 3 && !self.read_high;
                    if !self.read_high {
                        self.count_latch = None;
                    }
                    if high {
                        (count >> 8) as u8
                    } else {
                        count as u8
                    }
                }
                0x60 => {
                    let b = self.queue.pop_front().unwrap_or(0);
                    self.requests &= !2;
                    self.refresh_keyboard();
                    b
                }
                0x64 => 4 | u8::from(!self.queue.is_empty()),
                _ => return None,
            });
        }
        match p {
            0x20 if v & 0x10 != 0 => {
                self.init = 2;
                self.icw4 = v & 1 != 0;
                self.single = v & 2 != 0;
                self.service = 0;
                self.mask = 0;
            }
            0x20 if v & 8 != 0 => {
                if v & 2 != 0 {
                    self.read_service = v & 1 != 0;
                }
            }
            0x20 if v & 0x20 != 0 => {
                if v & 0x40 != 0 {
                    self.service &= !(1 << (v & 7));
                } else {
                    self.eoi()
                }
            }
            0x20 => {}
            0x21 => match self.init {
                2 => {
                    self.vector = v & 0xf8;
                    self.init = if !self.single {
                        3
                    } else if self.icw4 {
                        4
                    } else {
                        0
                    };
                }
                3 => self.init = if self.icw4 { 4 } else { 0 },
                4 => {
                    self.auto_eoi = v & 2 != 0;
                    self.init = 0
                }
                _ => self.mask = v,
            },
            0x43 if v >> 6 == 0 => {
                let access = (v >> 4) & 3;
                if access == 0 {
                    self.count_latch = Some((self.divisor - self.ticks) as u16)
                } else {
                    self.access = access;
                    self.timer_low = None;
                }
                self.read_high = false;
            }
            0x43 => {}
            0x40 => {
                let count = match self.access {
                    1 => u16::from(v),
                    2 => u16::from(v) << 8,
                    _ => {
                        if let Some(lo) = self.timer_low.take() {
                            u16::from_le_bytes([lo, v])
                        } else {
                            self.timer_low = Some(v);
                            return Some(0);
                        }
                    }
                };
                self.divisor = if count == 0 { 65536 } else { count.into() };
                self.ticks = 0;
            }
            0x64 => {
                self.command = v;
                match v {
                    0x20 => self.queue.push_back(self.command_byte),
                    0xaa => self.queue.push_back(0x55),
                    0xab => self.queue.push_back(0),
                    0xad => self.command_byte |= 16,
                    0xae => self.command_byte &= !16,
                    0xd0 => self.queue.push_back(1 | u8::from(*a20) << 1),
                    _ => {}
                }
                self.refresh_keyboard();
            }
            0x60 => {
                match core::mem::take(&mut self.command) {
                    0x60 => self.command_byte = v,
                    0xd1 => *a20 = v & 2 != 0,
                    _ => {
                        self.queue.push_back(0xfa);
                        if self.key_arg {
                            self.key_arg = false
                        } else {
                            match v {
                                0xed | 0xf0 | 0xf3 => self.key_arg = true,
                                0xff => self.queue.push_back(0xaa),
                                _ => {}
                            }
                        }
                    }
                }
                self.refresh_keyboard();
            }
            _ => return None,
        }
        Some(0)
    }
}
