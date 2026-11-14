//! IBM VGA mode 12h. Each aperture byte addresses eight pixels in four planes.
use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

struct Registers {
    seq: [u8; 8],
    gc: [u8; 16],
    attr: [u8; 32],
    crtc: [u8; 32],
    seq_index: u8,
    gc_index: u8,
    attr_index: u8,
    crtc_index: u8,
    attr_data: bool,
    misc: u8,
    dac_mask: u8,
    dac_write: u16,
    dac_read: u16,
    dac_state: u8,
    dac: [[u8; 3]; 256],
    clock: u32,
}
pub(super) struct Graphics {
    regs: RefCell<Registers>,
    planes: RefCell<Vec<[u8; 4]>>,
    latch: Cell<[u8; 4]>,
    pixels: Vec<u8>,
    dirty: Cell<bool>,
    pub active: bool,
    refresh: u16,
}
impl Graphics {
    pub fn new() -> Self {
        Self {
            regs: RefCell::new(Registers {
                seq: [0; 8],
                gc: [0; 16],
                attr: [0; 32],
                crtc: [0; 32],
                seq_index: 0,
                gc_index: 0,
                attr_index: 0,
                crtc_index: 0,
                attr_data: false,
                misc: 0x67,
                dac_mask: 255,
                dac_write: 0,
                dac_read: 0,
                dac_state: 0,
                dac: [[0; 3]; 256],
                clock: 0,
            }),
            planes: RefCell::new(vec![[0; 4]; 65536]),
            latch: Cell::new([0; 4]),
            pixels: Vec::new(),
            dirty: Cell::new(true),
            active: false,
            refresh: 0,
        }
    }
    pub fn write(&self, p: u16, v: u8) {
        self.port(p, true, v);
    }
    pub fn port(&self, p: u16, write: bool, v: u8) -> Option<u8> {
        let mut r = self.regs.borrow_mut();
        if !write {
            return Some(match p {
                0x3c0 => r.attr_index,
                0x3c1 => r.attr[usize::from(r.attr_index & 31)],
                0x3c2 => 0x10,
                0x3cc => r.misc,
                0x3c4 => r.seq_index,
                0x3c5 => r.seq[usize::from(r.seq_index & 7)],
                0x3c6 => r.dac_mask,
                0x3c7 => r.dac_state,
                0x3c8 => (r.dac_write / 3) as u8,
                0x3c9 => {
                    let i = usize::from(r.dac_read);
                    r.dac_read = (r.dac_read + 1) % 768;
                    r.dac[i / 3][i % 3]
                }
                0x3ce => r.gc_index,
                0x3cf => r.gc[usize::from(r.gc_index & 15)],
                0x3d4 | 0x3b4 => r.crtc_index,
                0x3d5 | 0x3b5 => r.crtc[usize::from(r.crtc_index & 31)],
                0x3da | 0x3ba => {
                    r.attr_data = false;
                    r.clock = r.clock.wrapping_add(9);
                    if self.active {
                        let y = r.clock / 100 % 525;
                        u8::from(y >= 480 || r.clock % 100 >= 80)
                            | u8::from((490..492).contains(&y)) << 3
                    } else {
                        super::memory::text_status(r.clock)
                    }
                }
                _ => return None,
            });
        }
        match p {
            0x3c0 => {
                if r.attr_data {
                    let i = usize::from(r.attr_index & 31);
                    r.attr[i] = v;
                } else {
                    r.attr_index = v;
                }
                r.attr_data = !r.attr_data;
            }
            0x3c2 => r.misc = v,
            0x3c4 => r.seq_index = v,
            0x3c5 => {
                let i = usize::from(r.seq_index & 7);
                r.seq[i] = v;
            }
            0x3c6 => r.dac_mask = v,
            0x3c7 => {
                r.dac_read = u16::from(v) * 3;
                r.dac_state = 3;
            }
            0x3c8 => {
                r.dac_write = u16::from(v) * 3;
                r.dac_state = 0;
            }
            0x3c9 => {
                let i = usize::from(r.dac_write);
                r.dac[i / 3][i % 3] = v & 63;
                r.dac_write = (r.dac_write + 1) % 768;
            }
            0x3ce => r.gc_index = v,
            0x3cf => {
                let i = usize::from(r.gc_index & 15);
                r.gc[i] = v;
            }
            0x3d4 | 0x3b4 => r.crtc_index = v,
            0x3d5 | 0x3b5 => {
                let i = usize::from(r.crtc_index & 31);
                r.crtc[i] = v;
            }
            _ => return None,
        }
        self.dirty.set(true);
        Some(0)
    }
    pub fn byte(&self, addr: usize) -> u8 {
        let planes = self.planes.borrow()[addr & 0xffff];
        self.latch.set(planes);
        let r = self.regs.borrow();
        if r.gc[5] & 8 == 0 {
            return planes[usize::from(r.gc[4] & 3)];
        }
        let mut matching = 255;
        for (i, b) in planes.into_iter().enumerate() {
            if r.gc[7] & (1 << i) != 0 {
                matching &= if r.gc[2] & (1 << i) != 0 { b } else { !b };
            }
        }
        matching
    }
    pub fn store(&self, addr: usize, v: u8) {
        let r = self.regs.borrow();
        let mode = r.gc[5] & 3;
        let rotated = v.rotate_right(u32::from(r.gc[3] & 7));
        let mask = if mode == 3 {
            rotated & r.gc[8]
        } else {
            r.gc[8]
        };
        let latch = self.latch.get();
        let mut planes = self.planes.borrow_mut();
        let dest = &mut planes[addr & 0xffff];
        for i in 0..4 {
            let bit = 1 << i;
            if r.seq[2] & bit == 0 {
                continue;
            }
            let fill = |enabled: bool| if enabled { 255 } else { 0 };
            let source = match mode {
                2 => fill(v & bit != 0),
                3 => fill(r.gc[0] & bit != 0),
                _ if r.gc[1] & bit != 0 => fill(r.gc[0] & bit != 0),
                _ => rotated,
            };
            let alu = match (r.gc[3] >> 3) & 3 {
                1 => source & latch[i],
                2 => source | latch[i],
                3 => source ^ latch[i],
                _ => source,
            };
            let value = if mode == 1 {
                latch[i]
            } else {
                (alu & mask) | (latch[i] & !mask)
            };
            if dest[i] != value {
                dest[i] = value;
                self.dirty.set(true);
            }
        }
    }
    pub fn attribute(&self, index: u8, value: u8) {
        let _ = self.port(0x3da, false, 0);
        self.write(0x3c0, index);
        self.write(0x3c0, value);
        self.write(0x3c0, 0x20);
    }
    fn indexed(&self, port: u16, index: u8, value: u8) {
        self.write(port, index);
        self.write(port + 1, value);
    }
    pub fn planar_mode(&mut self, clear: bool) {
        // IBM VGA mode 12h, 640 x 480, four independently addressable bit planes.
        const CRTC: [u8; 25] = [
            0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0x0b, 0x3e, 0, 0x40, 0, 0, 0, 0, 0, 0, 0xea, 0x8c,
            0xdf, 0x28, 0, 0xe7, 4, 0xe3, 0xff,
        ];
        self.write(0x3c2, 0xe3);
        for (index, value) in [3, 1, 15, 0, 6].into_iter().enumerate() {
            self.indexed(0x3c4, index as u8, value);
        }
        self.indexed(0x3d4, 0x11, 0);
        for (index, value) in CRTC.into_iter().enumerate() {
            self.indexed(0x3d4, index as u8, value);
        }
        for (index, value) in [0, 0, 0, 0, 0, 0, 5, 15, 255].into_iter().enumerate() {
            self.indexed(0x3ce, index as u8, value);
        }
        let _ = self.port(0x3da, false, 0);
        for (index, value) in [
            0, 1, 2, 3, 4, 5, 0x14, 7, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 1, 0, 15, 0,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            self.write(0x3c0, index as u8);
            self.write(0x3c0, value);
        }
        self.write(0x3c0, 0x20);
        self.write(0x3c6, 255);
        // The standard 64-entry EGA DAC cube (two bits per channel).
        self.write(0x3c8, 0);
        for i in 0..64u8 {
            for bit in [4, 2, 1] {
                let component = u8::from(i & bit != 0) * 42 + u8::from(i & (bit << 3) != 0) * 21;
                self.write(0x3c9, component);
            }
        }
        if clear {
            self.planes.get_mut().fill([0; 4]);
        }
        self.active = true;
        self.refresh = 0;
    }
    pub fn render(&mut self, force: bool) {
        if !self.active {
            return;
        }
        if !force && self.refresh > 0 {
            self.refresh -= 1;
            return;
        }
        self.refresh = 128;
        if !self.dirty.replace(false) && !force {
            return;
        }
        let r = self.regs.get_mut();
        let planes = self.planes.get_mut();
        let palette: [[_; 4]; 16] = core::array::from_fn(|i| {
            let entry = r.attr[i & usize::from(r.attr[0x12] & 15)];
            let high = if r.attr[16] & 128 != 0 {
                (r.attr[20] & 3) << 4
            } else {
                entry & 0x30
            };
            let idx = ((entry & 15) | high | ((r.attr[20] & 12) << 4)) & r.dac_mask;
            let [red, green, blue] = r.dac[usize::from(idx)].map(|x| (x << 2) | (x >> 4));
            [red, green, blue, 255]
        });
        self.pixels.resize(640 * 480 * 4, 0);
        let start = usize::from(u16::from_be_bytes([r.crtc[12], r.crtc[13]]));
        let stride = usize::from(r.crtc[19]) * 2;
        for (y, row) in self.pixels.chunks_exact_mut(640 * 4).enumerate() {
            for (x, group) in row.chunks_exact_mut(32).enumerate() {
                let p = planes[(start + y * stride + x) & 65535];
                for (bit, pixel) in group.chunks_exact_mut(4).enumerate() {
                    let idx = p.iter().enumerate().fold(0, |idx, (i, b)| {
                        idx | usize::from((b >> (7 - bit)) & 1) << i
                    });
                    pixel.copy_from_slice(&palette[idx]);
                }
            }
        }
        crate::bios::screen::graphics(640, 480, &self.pixels);
    }
}
