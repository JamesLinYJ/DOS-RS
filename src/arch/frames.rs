//! Ownership of firmware-donated physical pages used by native processes.
mod bitmap;

use crate::kernel_cell::KernelCell;
use core::sync::atomic::{AtomicU8, Ordering};
use dosrs_native_exec::{
    ANON_BASE, ANON_END, BASE, END, Frame, Image, PAGE, Result, STACK_LIMIT, STACK_TOP,
};

static FRAMES: KernelCell<bitmap::FreeFrames> = KernelCell::new(bitmap::FreeFrames::new());

/// Called once on the BSP before exposing the frame pool to processes.
/// Regions are (physical start, bytes, allocatable), with reservations winning.
pub(crate) fn initialize<I>(regions: I, reserved: &[(u64, u64)])
where
    I: Iterator<Item = (u64, u64, bool)> + Clone,
{
    let mut frames = FRAMES.borrow();
    for (start, length, _) in regions.clone().filter(|r| r.2) {
        frames.region(start, length, true);
    }
    for (start, length, _) in regions.filter(|r| !r.2) {
        frames.region(start, length, false);
    }
    // Low firmware pages include the IVT, option ROMs and AP startup code.
    frames.region(0, 1 << 20, false);
    for &(start, end) in &[(BASE, END), (STACK_LIMIT, STACK_TOP), (ANON_BASE, ANON_END)] {
        frames.region(start as u64, (end - start) as u64, false);
    }
    for &(start, length) in reserved {
        frames.region(start, length, false);
    }
}

/// (Total donated pages, pages currently free).
pub(crate) fn counts() -> (usize, usize) {
    let frames = FRAMES.borrow();
    (frames.total, frames.free)
}

pub(crate) type UserImage = Image<PhysicalFrame>;
pub(crate) struct PhysicalFrame(usize);

impl Frame for PhysicalFrame {
    fn allocate_zeroed() -> Result<Self> {
        let address = FRAMES
            .borrow()
            .take()
            .ok_or("insufficient physical memory")?;
        // The allocator granted exclusive ownership and no page table
        // currently maps this frame into a process.
        unsafe {
            (address as *mut u8).write_bytes(0, PAGE);
        }
        Ok(Self(address))
    }

    fn available() -> usize {
        counts().1
    }

    fn frame_address(&self) -> usize {
        self.0
    }

    fn read(&self, offset: usize, out: &mut [u8]) {
        assert!(offset <= PAGE && out.len() <= PAGE - offset);
        for (i, byte) in out.iter_mut().enumerate() {
            // Kernel access to live user memory must not create a borrowed
            // byte slice aliased by loads and stores on a sibling vCPU.
            let p = (self.0 + offset + i) as *mut u8;
            *byte = unsafe { AtomicU8::from_ptr(p) }.load(Ordering::Relaxed);
        }
    }

    fn write(&mut self, offset: usize, input: &[u8]) {
        assert!(offset <= PAGE && input.len() <= PAGE - offset);
        for (i, byte) in input.iter().enumerate() {
            let p = (self.0 + offset + i) as *mut u8;
            unsafe { AtomicU8::from_ptr(p) }.store(*byte, Ordering::Relaxed);
        }
    }
}

impl Drop for PhysicalFrame {
    fn drop(&mut self) {
        // The address-space owner retired the old CR3 on all CPUs first.
        FRAMES.borrow().release(self.0);
    }
}
