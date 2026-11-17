//! One physical pool. XMS owns KiB units, VCPI owns aligned 4 KiB frames,
//! and EMS owns aligned 16 KiB pages. The two bitmaps distinguish VCPI frees.
use super::{get, load_base, put};
pub(super) const EMS_FRAME: usize = 0xd0000;
pub(super) const EMS_PAGE: usize = 16384;
pub(super) const EXT_START: usize = 0x120000;
pub(super) const RAM_BYTES: usize = EXT_START + 32 * 1024 * 1024;
const UNITS: usize = (RAM_BYTES - EXT_START) / 1024;
pub(super) struct GuestRam {
    pub frame: [Option<usize>; 4],
    used: [u32; UNITS / 32],
    vcpi: [u32; UNITS / 128],
    units: usize,
    ems_capacity: usize,
    pub a20: bool,
}
impl GuestRam {
    pub const fn new() -> Self {
        Self {
            frame: [None; 4],
            used: [0; UNITS / 32],
            vcpi: [0; UNITS / 128],
            units: 0,
            ems_capacity: 0,
            a20: true,
        }
    }
    pub fn init(&mut self, end: usize) {
        self.units = (end.min(RAM_BYTES).saturating_sub(EXT_START)) / 1024;
    }
    pub fn highest_address(&self) -> usize {
        EXT_START + self.units * 1024 - 1
    }
    pub fn finish_reservations(&mut self) {
        self.ems_capacity = self.free_ems();
    }
    pub fn total_ems(&self) -> usize {
        self.ems_capacity
    }
    pub fn ems_return(&self) -> (u16, u16) {
        ((load_base() >> 4) as u16, 0x36)
    }
    fn occupied(&self, n: usize) -> bool {
        self.used[n / 32] & (1 << (n % 32)) != 0
    }
    fn holes(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let mut i = 0;
        core::iter::from_fn(move || {
            while i < self.units && self.occupied(i) {
                i += 1
            }
            let start = i;
            while i < self.units && !self.occupied(i) {
                i += 1
            }
            (start < i).then_some((EXT_START + 1024 * start, (i - start) * 1024))
        })
    }
    pub fn mark(&mut self, at: usize, len: usize, used: bool) {
        let start = (at - EXT_START) / 1024;
        for n in start..start + len / 1024 {
            let bit = 1 << (n % 32);
            if used {
                self.used[n / 32] |= bit
            } else {
                self.used[n / 32] &= !bit
            }
        }
    }
    pub fn xms_free_bytes(&self) -> (usize, usize) {
        self.holes().fold((0, 0), |(largest, total), (_, len)| {
            (largest.max(len), total + len)
        })
    }
    pub fn zero_ext(&mut self, at: usize, len: usize) {
        if len != 0 {
            unsafe {
                core::ptr::write_bytes(super::address(at as u32), 0, len);
            }
        }
    }
    pub fn allocate_ext(&mut self, len: usize) -> Option<usize> {
        if len == 0 {
            return Some(EXT_START);
        }
        let (at, _) = self.holes().find(|(_, space)| *space >= len)?;
        self.mark(at, len, true);
        Some(at)
    }
    fn free_groups(&self, kib: usize) -> impl DoubleEndedIterator<Item = usize> + '_ {
        (0..self.units / kib).filter(move |n| (0..kib).all(|j| !self.occupied(n * kib + j)))
    }
    pub fn free_ems(&self) -> usize {
        self.free_groups(16).count()
    }
    pub fn allocate_ems(&mut self, count: u16, mut assign: impl FnMut(usize, u16)) {
        let mut next = 0;
        for n in (0..self.units / 16).rev() {
            if next == count {
                break;
            }
            if (0..16).all(|j| !self.occupied(n * 16 + j)) {
                self.mark(EXT_START + n * EMS_PAGE, EMS_PAGE, true);
                self.zero_ext(EXT_START + n * EMS_PAGE, EMS_PAGE);
                assign(n, next);
                next += 1;
            }
        }
    }
    pub fn vcpi_free(&self) -> u32 {
        self.free_groups(4).count() as u32
    }
    pub fn alloc_frame(&mut self) -> Option<u32> {
        let n = self.free_groups(4).next_back()?;
        let addr = EXT_START + n * 4096;
        self.mark(addr, 4096, true);
        self.vcpi[n / 32] |= 1 << (n % 32);
        // A newly handed-out page may become a page table. Clear stale data before publication.
        self.zero_ext(addr, 4096);
        Some(addr as u32)
    }
    pub fn release_frame(&mut self, addr: u32) -> bool {
        let addr = addr as usize;
        if addr < EXT_START || addr > self.highest_address() || addr % 4096 != 0 {
            return false;
        }
        let n = (addr - EXT_START) / 4096;
        if self.vcpi[n / 32] & (1 << (n % 32)) == 0 {
            return false;
        }
        self.vcpi[n / 32] &= !(1 << (n % 32));
        self.mark(addr, 4096, false);
        true
    }
    pub fn bus_byte(&self, addr: usize) -> u8 {
        get(addr as u32)
    }
    pub fn set_bus_byte(&mut self, addr: usize, b: u8) {
        put(addr as u32, b)
    }
    pub fn load(&self, s: u16, o: u16) -> Option<u8> {
        Some(get(s as u32 * 16 + o as u32))
    }
    pub fn store(&mut self, s: u16, o: u16, v: u8) -> bool {
        put(s as u32 * 16 + o as u32, v);
        true
    }
    pub fn move_physical(&mut self, src: usize, dst: usize, n: usize) -> Option<()> {
        if src.checked_add(n)? > self.highest_address() + 1
            || dst.checked_add(n)? > self.highest_address() + 1
        {
            return None;
        }
        let hma = !self.a20
            && [src, dst]
                .iter()
                .any(|&at| at < 0x110000 && at + n > 0x100000);
        if hma {
            self.a20 = true;
            super::install_mapping(self);
        }
        if n != 0 {
            unsafe { core::ptr::copy(super::address(src as u32), super::address(dst as u32), n) }
        }
        if hma {
            self.a20 = false;
            super::install_mapping(self);
        }
        Some(())
    }
}
