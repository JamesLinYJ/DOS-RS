//! Shared byte-range locks for DOS open descriptions.
use crate::kernel_cell::KernelCell;
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct FileKey(pub (u32, u64));
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Owner {
    Guest { terminal: usize, description: usize },
    Gate(u16),
}
#[derive(Clone, Copy)]
struct Lock {
    file: FileKey,
    owner: Owner,
    start: u32,
    len: u32,
}

impl Lock {
    fn overlaps(self, start: u32, len: u32) -> bool {
        self.len != 0
            && len != 0
            && u64::from(self.start) < u64::from(start) + u64::from(len)
            && u64::from(start) < u64::from(self.start) + u64::from(self.len)
    }
}

static LOCKS: KernelCell<[Option<Lock>; 32]> = KernelCell::new([None; 32]);

pub(crate) fn discard(mut closed: impl FnMut(Owner) -> bool) {
    for entry in LOCKS.borrow().iter_mut() {
        if entry.is_some_and(|lock| closed(lock.owner)) {
            *entry = None
        }
    }
}
pub(crate) fn blocked(file: FileKey, owner: Owner, start: u32, len: u32) -> bool {
    LOCKS
        .borrow()
        .iter()
        .flatten()
        .any(|l| l.file == file && l.owner != owner && l.overlaps(start, len))
}
pub(crate) fn change(file: FileKey, owner: Owner, op: u8, start: u32, len: u32) -> Option<u16> {
    let mut locks = LOCKS.borrow();
    match op {
        0 if locks
            .iter()
            .flatten()
            .any(|l| l.file == file && l.overlaps(start, len)) =>
        {
            Some(0x21)
        }
        0 => match locks.iter_mut().find(|l| l.is_none()) {
            Some(slot) => {
                *slot = Some(Lock {
                    file,
                    owner,
                    start,
                    len,
                });
                None
            }
            None => Some(0x24),
        },
        1 => match locks.iter_mut().find(|v| {
            v.is_some_and(|l| {
                l.file == file && l.owner == owner && l.start == start && l.len == len
            })
        }) {
            Some(slot) => {
                *slot = None;
                None
            }
            None => Some(0x21),
        },
        _ => Some(1),
    }
}
