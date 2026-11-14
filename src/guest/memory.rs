//! Checked guest RAM.
//!
//! Each session has 640 KiB of conventional memory, a VGA/ROM window,
//! an A20 gate and configurable extended RAM. All addresses are guest offsets.
#[cfg(not(test))]
pub const RAM_BYTES: usize = include!(concat!(env!("OUT_DIR"), "/guest-ram-bytes.rs"));
#[cfg(test)]
pub const RAM_BYTES: usize = 9 * 1024 * 1024;
const EMS_MASK: u64 = (1 << (EMS_PAGE / KIB)) - 1;

pub(super) const EXT_START: usize = 0x110000;
pub(super) const KIB: usize = 1024;
const EXT_UNITS: usize = (RAM_BYTES - EXT_START) / KIB;
pub(super) const EMS_PAGE: usize = 0x4000;
pub(super) const EMS_FRAME: usize = 0xd0000;

/// First paragraph past conventional memory. 640KiB.
pub const CONVENTIONAL_END: u16 = 0xA000;

pub struct GuestRam {
    #[cfg(feature = "pc386")]
    changed: [u64; RAM_BYTES.div_ceil(4096 * 64)],
    #[cfg(feature = "pc386")]
    pub(super) machine_memory: Option<alloc::sync::Arc<rsemu::core::space::RamStore>>,
    #[cfg(feature = "pc386")]
    pub(super) at: Option<super::at::At>,
    #[cfg(feature = "pc386")]
    pub(super) graphics: Option<super::vga::Graphics>,
    bytes: [u8; RAM_BYTES],
    pub(crate) a20: bool,
    units: [u64; EXT_UNITS.div_ceil(64)],
    pub(super) frame: [Option<usize>; 4],
    dirty_rows: u32,
    pub(super) raster_clock: u32,
    kbc_command: u8,
    kbc_output: Option<u8>,
}

static STORAGE: [crate::kernel_cell::KernelCell<GuestRam>; 4] = [const {
    crate::kernel_cell::KernelCell::new(GuestRam {
        #[cfg(feature = "pc386")]
        changed: [0; RAM_BYTES.div_ceil(4096 * 64)],
        #[cfg(feature = "pc386")]
        machine_memory: None,
        #[cfg(feature = "pc386")]
        graphics: None,
        #[cfg(feature = "pc386")]
        at: None,
        bytes: [0; RAM_BYTES],
        units: [0; EXT_UNITS.div_ceil(64)],
        frame: [None; 4],
        a20: false,
        dirty_rows: 0,
        raster_clock: 0,
        kbc_command: 0,
        kbc_output: None,
    })
}; 4];

pub(crate) fn storage() -> crate::kernel_cell::Guard<'static, GuestRam> {
    session_ram(0)
}
pub(crate) fn session_ram(id: usize) -> crate::kernel_cell::Guard<'static, GuestRam> {
    STORAGE[id].borrow()
}

pub fn real_linear(segment: u16, offset: u16) -> Option<u32> {
    let sum = u32::from(segment)
        .checked_mul(16)?
        .checked_add(u32::from(offset))?;
    Some(sum & 0x000F_FFFF)
}

impl GuestRam {
    #[cfg(feature = "pc386")]
    fn touch(&mut self, start: usize, end: usize) {
        for page in start / 4096..end.div_ceil(4096) {
            self.changed[page / 64] |= 1 << (page % 64);
        }
    }
    #[cfg(feature = "pc386")]
    pub(super) fn upload(&mut self, mem: &rsemu::core::space::RamStore) {
        for (group, mask) in self.changed.iter_mut().enumerate() {
            while *mask != 0 {
                let n = mask.trailing_zeros() as usize;
                *mask &= !(1 << n);
                let at = (group * 64 + n) * 4096;
                if at < RAM_BYTES {
                    mem.write_at(at as u64, &self.bytes[at..at + 4096]).unwrap();
                }
            }
        }
        mem.clear_dirty();
    }
    #[cfg(feature = "pc386")]
    pub(super) fn download(&mut self, mem: &rsemu::core::space::RamStore) {
        mem.for_each_dirty_page(|page| {
            let at = page as usize * 4096;
            if at == 0xb8000 {
                self.dirty_rows = 0x01ffffff;
            }
            mem.read_at(at as u64, &mut self.bytes[at..at + 4096])
                .unwrap();
        });
        mem.clear_dirty();
    }
    pub fn clear(&mut self) {
        #[cfg(feature = "pc386")]
        {
            self.at = Some(super::at::At::new());
            self.graphics = Some(super::vga::Graphics::new());
            self.machine_memory = None;
            self.changed.fill(u64::MAX);
        }
        self.units.fill(0);
        self.frame = [None; 4];
        self.bytes.fill(0);
        self.a20 = false;
        self.raster_clock = 0;
        self.kbc_command = 0;
        self.kbc_output = None;
        // Microsoft C walks a message list at FFFF:0758. That address folds
        // to 0748 here. A word of all ones is the empty list, so the walk ends.
        // BIOS Data Area and the conventional AT configuration table.
        self.bytes[0x410..0x412].copy_from_slice(&0x0421u16.to_le_bytes());
        self.bytes[0x413..0x415].copy_from_slice(&640u16.to_le_bytes());
        self.dirty_rows = 0;
        for pair in self.bytes[0xb8000..0xb8000 + 4000].chunks_exact_mut(2) {
            pair.copy_from_slice(&[32, 7]);
        }
        // XMS 3.0 publishes a writable five-byte jump slot. Memory managers
        // can chain a far jump here and call the original control entry.
        self.bytes[0xf0100..0xf0105].copy_from_slice(&[0xeb, 3, 0x90, 0x90, 0x90]);
        self.bytes[0x67 * 4..0x67 * 4 + 4].copy_from_slice(&[0, 0, 0, 0xf1]);
        self.bytes[0xf1000 + 10..0xf1000 + 18].copy_from_slice(b"EMMXXXX0");
        #[cfg(feature = "pc386")]
        for vector in [
            0x08u8, 0x09, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x1a, 0x1c, 0x20, 0x21,
            0x25, 0x26, 0x28, 0x2a, 0x2e, 0x2f, 0x33,
        ] {
            let slot = usize::from(vector) * 4;
            let offset = 0x200 + u16::from(vector) * 2;
            self.bytes[slot..slot + 2].copy_from_slice(&offset.to_le_bytes());
            self.bytes[slot + 2..slot + 4].copy_from_slice(&0xf000u16.to_le_bytes());
            self.bytes[0xf0000 + usize::from(offset)] = 0xcf;
        }
        #[cfg(not(test))]
        for ch in 0..=255u8 {
            let rows = crate::bios::screen::glyph(ch);
            for (i, pattern) in rows.into_iter().enumerate() {
                let bits = pattern.reverse_bits();
                self.bytes[0xf4000 + usize::from(ch) * 8 + i] = bits;
                let at = 0xf4800 + usize::from(ch) * 16 + i * 2;
                self.bytes[at..at + 2].fill(bits);
            }
        }
        self.bytes[0x1f * 4..0x1f * 4 + 4].copy_from_slice(&[0, 0x44, 0, 0xf0]);
        self.bytes[0x43 * 4..0x43 * 4 + 4].copy_from_slice(&[0, 0x48, 0, 0xf0]);
        self.bytes[0x46c] = 1;
        self.bytes[0x449] = 3;
        self.bytes[0x44a..0x44c].copy_from_slice(&80u16.to_le_bytes());
        self.bytes[0x44c..0x44e].copy_from_slice(&4096u16.to_le_bytes());
        self.bytes[0x460..0x462].copy_from_slice(&[7, 6]);
        self.bytes[0x463..0x465].copy_from_slice(&0x3d4u16.to_le_bytes());
        self.bytes[0x465] = 0x29;
        self.bytes[0x484] = 24;
        self.bytes[0x485..0x487].copy_from_slice(&16u16.to_le_bytes());
        self.bytes[0x487] = 0x60;
        self.bytes[0x488] = 0xf9;
        self.bytes[0x489] = 0x51;
        self.bytes[0x48a] = 8;
        self.bytes[0xfe6f5..0xfe6ff].copy_from_slice(&[8, 0, 0xfc, 0, 1, 0, 0, 0, 0, 0]);
        self.bytes[0x748] = 0xFF;
        self.bytes[0x749] = 0xFF;
    }

    /// A single bitmap accounts for XMS blocks and aligned 16 KiB EMS pages.
    fn free_runs(&self, mut visit: impl FnMut(usize, usize)) {
        let mut start = 0;
        for i in 0..=EXT_UNITS {
            if i == EXT_UNITS || self.units[i / 64] & (1u64 << (i % 64)) != 0 {
                if start < i {
                    visit(EXT_START + start * KIB, (i - start) * KIB);
                }
                start = i + 1;
            }
        }
    }
    pub(super) fn xms_free_bytes(&self) -> (usize, usize) {
        let (mut largest, mut total) = (0, 0);
        self.free_runs(|_, bytes| {
            largest = largest.max(bytes);
            total += bytes;
        });
        (largest, total)
    }

    pub(super) fn mark(&mut self, start: usize, len: usize, used: bool) {
        let unit = (start - EXT_START) / KIB;
        for i in unit..unit + len / KIB {
            let bit = 1u64 << (i % 64);
            if used {
                self.units[i / 64] |= bit;
            } else {
                self.units[i / 64] &= !bit;
            }
        }
    }
    pub(super) fn allocate_ext(&mut self, len: usize) -> Option<usize> {
        if len == 0 {
            return Some(EXT_START);
        }
        let mut first = None;
        self.free_runs(|start, room| {
            if first.is_none() && room >= len {
                first = Some(start);
            }
        });
        let start = first?;
        self.mark(start, len, true);
        Some(start)
    }
    pub(super) fn free_ems(&self) -> usize {
        self.units
            .iter()
            .map(|word| {
                (0..4)
                    .filter(|i| (*word >> (16 * i)) & EMS_MASK == 0)
                    .count()
            })
            .sum()
    }
    /// Allocate in descending physical order, leaving low memory contiguous for XMS.
    /// The caller first reserves enough free pages under the same exclusive RAM borrow.
    pub(super) fn allocate_ems(&mut self, count: u16, mut assign: impl FnMut(usize, u16)) {
        let mut n = 0;
        for (w, word) in self.units.iter_mut().enumerate().rev() {
            for group in (0..4).rev() {
                if n == count {
                    return;
                }
                let mask = EMS_MASK << (group * 16);
                if *word & mask == 0 {
                    *word |= mask;
                    assign(w * 4 + group, n);
                    n += 1;
                }
            }
        }
        debug_assert_eq!(n, count);
    }
    fn mapped(&self, linear: usize) -> usize {
        if (EMS_FRAME..EMS_FRAME + 4 * EMS_PAGE).contains(&linear) {
            let at = linear - EMS_FRAME;
            if let Some(base) = self.frame[at / EMS_PAGE] {
                return base + at % EMS_PAGE;
            }
        }
        linear
    }
    pub(super) fn bus_byte(&self, linear: usize) -> u8 {
        #[cfg(feature = "pc386")]
        if (0xa0000..0xb0000).contains(&linear)
            && let Some(g) = &self.graphics
        {
            return g.byte(linear);
        }
        self.bytes[self.mapped(linear)]
    }
    pub(super) fn set_bus_byte(&mut self, linear: usize, value: u8) {
        #[cfg(feature = "pc386")]
        if (0xa0000..0xb0000).contains(&linear)
            && let Some(g) = &self.graphics
        {
            g.store(linear, value);
            return;
        }
        let at = self.mapped(linear);
        if (0xb8000..0xb8000 + 4000).contains(&at) && self.bytes[at] != value {
            self.dirty_rows |= 1 << ((at - 0xb8000) / 160);
        }
        #[cfg(feature = "pc386")]
        self.touch(at, at + 1);
        self.bytes[at] = value;
    }
    pub(super) fn move_physical(&mut self, src: usize, dst: usize, len: usize) -> Option<()> {
        if src.checked_add(len)? > RAM_BYTES || dst.checked_add(len)? > RAM_BYTES {
            return None;
        }
        if src >= EXT_START && dst >= EXT_START {
            #[cfg(feature = "pc386")]
            self.touch(dst, dst + len);
            self.bytes.copy_within(src..src + len, dst);
        } else if dst > src && dst - src < len {
            for i in (0..len).rev() {
                let v = self.bus_byte(src + i);
                self.set_bus_byte(dst + i, v);
            }
        } else {
            for i in 0..len {
                let v = self.bus_byte(src + i);
                self.set_bus_byte(dst + i, v);
            }
        }
        Some(())
    }
    pub(super) fn address(&self, seg: u16, off: u16) -> usize {
        if self.a20 {
            usize::from(seg) * 16 + usize::from(off)
        } else {
            real_linear(seg, off).unwrap() as usize
        }
    }
    pub(super) fn system_port(&mut self, port: u16, write: bool, value: u8) -> Option<u8> {
        #[cfg(feature = "pc386")]
        if let Some(g) = &self.graphics
            && let Some(result) = g.port(port, write, value)
        {
            return Some(result);
        }
        match (port, write) {
            (0x3da, false) if self.bytes[0x463] == 0xd4 => {
                self.raster_clock = self.raster_clock.wrapping_add(9);
                Some(text_status(self.raster_clock))
            }
            (0x92, false) => Some(u8::from(self.a20) << 1),
            (0x92, true) => {
                self.a20 = value & 2 != 0;
                Some(0)
            }
            (0x64, false) => Some(u8::from(self.kbc_output.is_some())),
            (0x64, true) => {
                self.kbc_command = value;
                if value == 0xd0 {
                    self.kbc_output = Some(1 | u8::from(self.a20) << 1);
                }
                Some(0)
            }
            (0x60, false) => Some(self.kbc_output.take().unwrap_or(0)),
            (0x60, true) if self.kbc_command == 0xd1 => {
                self.a20 = value & 2 != 0;
                self.kbc_command = 0;
                Some(0)
            }
            _ => None,
        }
    }
    pub fn load(&self, segment: u16, offset: u16) -> Option<u8> {
        Some(self.bus_byte(self.address(segment, offset)))
    }

    pub fn store(&mut self, segment: u16, offset: u16, value: u8) -> bool {
        self.set_bus_byte(self.address(segment, offset), value);
        true
    }

    pub(super) fn text(&self, row: u8, col: u8) -> u16 {
        let at = 0xb8000 + 160 * usize::from(row.min(24)) + 2 * usize::from(col.min(79));
        u16::from_le_bytes([self.bytes[at], self.bytes[at + 1]])
    }
    pub(super) fn write_text(&mut self, row: u8, col: u8, word: u16) {
        let idx = 80 * usize::from(row.min(24)) + usize::from(col.min(79));
        let start = 0xb8000 + 2 * idx;
        #[cfg(feature = "pc386")]
        self.touch(start, start + 2);
        self.bytes[start..start + 2].copy_from_slice(&word.to_le_bytes());
        self.dirty_rows |= 1 << row.min(24);
    }
    pub(super) fn updated_text(&mut self, mut row: impl FnMut(usize, &[u8])) {
        let mut dirty = core::mem::take(&mut self.dirty_rows);
        while dirty != 0 {
            let y = dirty.trailing_zeros() as usize;
            dirty &= dirty - 1;
            let start = 0xb8000 + y * 160;
            row(y, &self.bytes[start..start + 160]);
        }
    }
    pub(super) fn paragraphs_mut(&mut self, start: u16, end: u16) -> &mut [u8] {
        #[cfg(feature = "pc386")]
        self.touch(usize::from(start) * 16, usize::from(end) * 16);
        &mut self.bytes[usize::from(start) * 16..usize::from(end) * 16]
    }
}

/// VGA mode 03h uses 100 character clocks per line and 449 lines per frame.
pub(super) fn text_status(clock: u32) -> u8 {
    let line = clock / 100 % 449;
    let blank = line >= 400 || clock % 100 >= 80;
    u8::from(blank) | (u8::from((412..414).contains(&line)) << 3)
}

pub(super) fn read_word(ram: &GuestRam, segment: u16, offset: u16) -> Option<u16> {
    let low = ram.load(segment, offset)?;
    let high = ram.load(segment, offset.wrapping_add(1))?;
    Some(u16::from_le_bytes([low, high]))
}
