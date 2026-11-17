//! Query scheduler eligibility by Linux-style task ID. Every scheduler queue
//! accepts every live user thread. CPU indices follow the kernel's online mask.
use crate::{arch::user::AddressSpace, kernel_cell::KernelCell};
use alloc::vec::Vec;

// The process table owns main-thread IDs. Secondary threads reserve a distinct
// ID while alive, including when blocked in another process.
static SECONDARY: KernelCell<Vec<u32>> = KernelCell::new(Vec::new());

pub(super) struct ActiveTid(u32);

impl ActiveTid {
    pub(super) fn register(tid: u32) -> Result<Self, &'static str> {
        let mut ids = SECONDARY.borrow();
        ids.try_reserve(1)
            .map_err(|_| "insufficient thread memory")?;
        ids.push(tid);
        Ok(Self(tid))
    }
}

impl Drop for ActiveTid {
    fn drop(&mut self) {
        let mut ids = SECONDARY.borrow();
        if let Some(index) = ids.iter().position(|id| *id == self.0) {
            ids.swap_remove(index);
        }
    }
}

pub(super) fn get(
    image: &mut AddressSpace,
    [target, size, dest]: [usize; 3],
) -> Result<usize, isize> {
    let tid = i32::try_from(target).map_err(|_| -3isize)? as u32;
    let mask = crate::platform::online_mask().to_ne_bytes();
    if size < mask.len() || !size.is_multiple_of(mask.len()) {
        return Err(-22);
    }
    if tid != 0 && !super::super::process::live_pid(tid) && !SECONDARY.borrow().contains(&tid) {
        return Err(-3);
    }
    image.write(dest, &mask).map_err(|_| -14isize)?;
    Ok(mask.len())
}
