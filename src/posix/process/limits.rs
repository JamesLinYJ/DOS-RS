//! Per-process resource limits. The image enforces main-stack growth.
use super::TABLE;
use crate::arch::user::AddressSpace;
use dosrs_native_exec::STACK_MAX;

#[derive(Clone, Copy)]
pub(super) struct Limits {
    stack_soft: u64,
    stack_hard: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            stack_soft: STACK_MAX as u64,
            stack_hard: STACK_MAX as u64,
        }
    }
}

pub(in crate::posix) fn stack(pid: u32) -> Result<usize, isize> {
    TABLE
        .borrow()
        .iter()
        .find(|p| p.pid == pid)
        .map(|p| p.limits.stack_soft as usize)
        .ok_or(-3)
}

pub(in crate::posix) fn call(
    image: &mut AddressSpace,
    pid: u32,
    [target, resource, pointers]: [usize; 3],
) -> Result<usize, isize> {
    if target != 0 && target != pid as usize {
        return Err(-95);
    }
    if resource != 3 {
        return Err(-22);
    }
    let mut words = [0; 2 * size_of::<usize>()];
    image.read(pointers, &mut words).map_err(|_| -14isize)?;
    let width = size_of::<usize>();
    let new = usize::from_le_bytes(words[..width].try_into().unwrap());
    let old = usize::from_le_bytes(words[width..].try_into().unwrap());
    let requested = if new == 0 {
        None
    } else {
        let mut bytes = [0; 16];
        image.read(new, &mut bytes).map_err(|_| -14isize)?;
        Some(Limits {
            stack_soft: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            stack_hard: u64::from_le_bytes(bytes[8..].try_into().unwrap()),
        })
    };
    if old != 0 {
        image.check(old, 16, true).map_err(|_| -14isize)?;
    }
    let mut table = TABLE.borrow();
    let process = table.iter_mut().find(|p| p.pid == pid).ok_or(-3isize)?;
    if let Some(limit) = requested {
        if limit.stack_soft > limit.stack_hard {
            return Err(-22);
        }
        if limit.stack_hard > STACK_MAX as u64
            || (limit.stack_hard > process.limits.stack_hard && process.uid != 0)
        {
            return Err(-1);
        }
    }
    if old != 0 {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&process.limits.stack_soft.to_le_bytes());
        bytes[8..].copy_from_slice(&process.limits.stack_hard.to_le_bytes());
        image.write(old, &bytes).map_err(|_| -14isize)?;
    }
    if let Some(limit) = requested {
        image
            .set_stack_limit(limit.stack_soft as usize)
            .map_err(|_| -22isize)?;
        process.limits = limit;
    }
    Ok(0)
}
