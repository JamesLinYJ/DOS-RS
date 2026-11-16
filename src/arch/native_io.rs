//! DPL0 INT 21h buffer transport. The native ABI caller owns the memory for the entire call.
use super::GateToken;
use alloc::vec::Vec;
impl GateToken {
    fn address(&self, addr: u64, len: usize) -> Option<usize> {
        let addr = usize::try_from(addr).ok()?;
        if addr == 0 || addr.checked_add(len).is_none() {
            return None;
        }
        Some(addr)
    }
    pub(crate) fn copy_in(&self, addr: u64, out: &mut [u8]) -> Option<()> {
        if out.is_empty() {
            return Some(());
        }
        let start = self.address(addr, out.len())?;
        // SAFETY: The supervisor-only gate takes a readable caller buffer of the ABI length.
        unsafe { core::ptr::copy(start as *const u8, out.as_mut_ptr(), out.len()) }
        Some(())
    }
    pub(crate) fn copy_out(&self, addr: u64, data: &[u8]) -> Option<()> {
        if data.is_empty() {
            return Some(());
        }
        let start = self.address(addr, data.len())?;
        // SAFETY: The supervisor caller grants write access for the indicated ABI result.
        unsafe { core::ptr::copy(data.as_ptr(), start as *mut u8, data.len()) }
        Some(())
    }
    pub(crate) fn pathname(&self, addr: u64) -> Option<Vec<u8>> {
        let mut path = Vec::new();
        for i in 0..260 {
            let mut byte = [0];
            self.copy_in(addr.checked_add(i)?, &mut byte)?;
            if byte[0] == 0 {
                return Some(path);
            }
            path.push(byte[0]);
        }
        None
    }
}
