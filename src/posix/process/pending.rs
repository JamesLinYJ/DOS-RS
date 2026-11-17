//! Pending signals keep the generating PID and credentials captured at send time.
use super::bit;
use alloc::vec::Vec;

#[derive(Clone, Copy, Default)]
pub(in crate::posix) struct Origin {
    pub code: i32,
    pub pid: u32,
    pub uid: u32,
    pub status: i32,
}
impl Origin {
    pub(super) const KERNEL: Self = Self {
        code: 128,
        pid: 0,
        uid: 0,
        status: 0,
    };
}
pub(in crate::posix) struct Pending {
    mask: u64,
    first: [Origin; 64],
    realtime: Vec<(u8, Origin)>,
}
impl Default for Pending {
    fn default() -> Self {
        Self {
            mask: 0,
            first: [Origin::KERNEL; 64],
            realtime: Vec::new(),
        }
    }
}
impl Pending {
    pub(in crate::posix) fn mask(&self) -> u64 {
        self.mask
    }
    pub(in crate::posix) fn add(&mut self, n: u8, origin: Origin) -> Result<(), isize> {
        if self.mask & bit(n) == 0 {
            self.first[usize::from(n - 1)] = origin;
            self.mask |= bit(n);
        } else if n >= 32 {
            if self.realtime.len() >= 128 {
                return Err(-11);
            }
            self.realtime.try_reserve(1).map_err(|_| -11isize)?;
            self.realtime.push((n, origin));
        }
        Ok(())
    }
    pub(super) fn clear(&mut self, mask: u64) {
        self.mask &= !mask;
        self.realtime.retain(|(n, _)| bit(*n) & mask == 0);
    }
    pub(in crate::posix) fn take(&mut self, eligible: u64) -> Option<(u8, Origin)> {
        let bits = self.mask & eligible;
        if bits == 0 {
            return None;
        }
        let n = if bits & bit(9) != 0 {
            9
        } else {
            bits.trailing_zeros() as u8 + 1
        };
        let slot = usize::from(n - 1);
        let first = self.first[slot];
        if n >= 32
            && let Some(next) = self.realtime.iter().position(|(s, _)| *s == n)
        {
            self.first[slot] = self.realtime.remove(next).1;
        } else {
            self.mask &= !bit(n);
        }
        Some((n, first))
    }
}
