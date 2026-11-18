//! Supplementary group IDs belong to the process and survive both fork and exec.
use crate::arch::user::AddressSpace;

// musl publishes this limit in limits.h and sysconf(_SC_NGROUPS_MAX).
const MAX: usize = 32;
#[derive(Clone, Copy, Default)]
pub(super) struct Groups {
    ids: [u32; MAX],
    used: usize,
}
impl Groups {
    pub(super) fn as_slice(&self) -> &[u32] {
        &self.ids[..self.used]
    }
    pub(super) fn get(
        &self,
        image: &mut AddressSpace,
        capacity: usize,
        addr: usize,
    ) -> Result<usize, isize> {
        if capacity > i32::MAX as usize || (capacity != 0 && capacity < self.used) {
            return Err(-22);
        }
        if capacity != 0 && self.used != 0 {
            let mut wire = [0; MAX * 4];
            for (word, id) in wire.chunks_exact_mut(4).zip(self.as_slice()) {
                word.copy_from_slice(&id.to_le_bytes());
            }
            image
                .write(addr, &wire[..self.used * 4])
                .map_err(|_| -14isize)?;
        }
        Ok(self.used)
    }
    pub(super) fn set(
        &mut self,
        image: &AddressSpace,
        uid: u32,
        count: usize,
        addr: usize,
    ) -> Result<usize, isize> {
        if uid != 0 {
            return Err(-1);
        }
        if count > MAX {
            return Err(-22);
        }
        let mut next = Self::default();
        if count != 0 {
            let mut wire = [0; MAX * 4];
            image
                .read(addr, &mut wire[..count * 4])
                .map_err(|_| -14isize)?;
            for (id, bytes) in next.ids.iter_mut().zip(wire[..count * 4].chunks_exact(4)) {
                *id = u32::from_le_bytes(bytes.try_into().unwrap());
                if *id == u32::MAX {
                    return Err(-22);
                }
            }
            next.ids[..count].sort_unstable();
        }
        next.used = count;
        *self = next;
        Ok(0)
    }
}
