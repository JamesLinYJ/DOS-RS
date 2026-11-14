//! Identity-mapped PCI apertures and exclusive, coherent x86 DMA pages.
use core::{
    cell::UnsafeCell,
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

static MMIO_MAX: AtomicUsize = AtomicUsize::new(u32::MAX as usize);
pub(crate) fn firmware_mmio() {
    MMIO_MAX.store(usize::MAX, Ordering::Relaxed);
}
#[derive(Clone, Copy)]
pub(crate) struct Registers {
    base: usize,
    len: usize,
}
impl Registers {
    /// Caller has claimed a firmware-assigned memory BAR of this size.
    pub(crate) unsafe fn from_bar(base: usize, len: usize) -> Option<Self> {
        (base >= 0x100000
            && base % 4 == 0
            && len >= 4
            && base
                .checked_add(len - 1)
                .is_some_and(|end| end <= MMIO_MAX.load(Ordering::Relaxed)))
        .then_some(Self { base, len })
    }
    pub(crate) fn read(self, offset: usize) -> u32 {
        assert!(offset % 4 == 0 && offset <= self.len - 4);
        unsafe { ptr::read_volatile((self.base + offset) as *const u32) }
    }
    pub(crate) fn write(self, offset: usize, value: u32) {
        assert!(offset % 4 == 0 && offset <= self.len - 4);
        unsafe {
            ptr::write_volatile((self.base + offset) as *mut u32, value);
        }
    }
    pub(crate) fn fits(self, offset: usize, length: usize) -> bool {
        offset
            .checked_add(length)
            .is_some_and(|end| end <= self.len)
    }
}

#[repr(C, align(4096))]
struct Page(UnsafeCell<[u32; 1024]>);
// Pages are assigned exactly once. Devices retain their pages for kernel lifetime.
unsafe impl Sync for Page {}
static PAGES: [Page; 256] = [const { Page(UnsafeCell::new([0; 1024])) }; 256];
static NEXT: AtomicUsize = AtomicUsize::new(0);
pub(crate) struct Dma(&'static Page);
impl Dma {
    pub(crate) fn new() -> Option<Self> {
        let n = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < PAGES.len()).then_some(n + 1)
            })
            .ok()?;
        let page = Self(&PAGES[n]);
        u32::try_from(page.0.0.get() as usize + 4095).ok()?;
        Some(page)
    }
    pub(crate) fn addr(&self) -> u32 {
        self.0.0.get() as usize as u32
    }
    pub(crate) fn read(&self, word: usize) -> u32 {
        assert!(word < 1024);
        unsafe { self.0.0.get().cast::<u32>().add(word).read_volatile() }
    }
    pub(crate) fn write(&mut self, word: usize, value: u32) {
        assert!(word < 1024);
        unsafe {
            self.0.0.get().cast::<u32>().add(word).write_volatile(value);
        }
    }
    pub(crate) fn send(&mut self, data: &[u8]) {
        assert!(data.len() <= 4096);
        for (i, part) in data.chunks(4).enumerate() {
            let mut bytes = [0; 4];
            bytes[..part.len()].copy_from_slice(part);
            self.write(i, u32::from_le_bytes(bytes));
        }
    }
    pub(crate) fn receive(&self, out: &mut [u8]) {
        assert!(out.len() <= 4096);
        for (i, part) in out.chunks_mut(4).enumerate() {
            part.copy_from_slice(&self.read(i).to_le_bytes()[..part.len()]);
        }
    }
}
