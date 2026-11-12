//! DOS byte-range locks shared by all sessions on the mounted volume.
use super::{CpuState, Session, Slot, fail};
use crate::kernel_cell::KernelCell;

#[derive(Clone, Copy, Eq, PartialEq)]
struct FileKey {
    directory: u32,
    name: [u8; 11],
}
#[derive(Clone, Copy, Eq, PartialEq)]
struct Owner {
    terminal: usize,
    description: usize,
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

fn identity(session: &Session, h: u16) -> Result<(FileKey, Owner), u16> {
    let file = session.handles.get(h).ok_or(6u16)?;
    if !file.named {
        return Err(1);
    }
    Ok((
        FileKey {
            directory: u32::from(file.dir),
            name: file.name,
        },
        Owner {
            terminal: session.terminal,
            description: session.handles.slot(h).unwrap(),
        },
    ))
}

pub(super) fn discard_closed(session: &Session) {
    for entry in LOCKS.borrow().iter_mut() {
        if entry.is_some_and(|l| {
            l.owner.terminal == session.terminal && !session.handles.live(l.owner.description)
        }) {
            *entry = None;
        }
    }
}

pub(super) fn blocked(session: &Session, h: u16, start: u32, len: u32) -> bool {
    let Ok((file, owner)) = identity(session, h) else {
        return false;
    };
    LOCKS
        .borrow()
        .iter()
        .flatten()
        .any(|l| l.owner != owner && l.file == file && l.overlaps(start, len))
}

pub(super) fn service(cpu: &mut CpuState, session: &mut Session) {
    let (file, owner) = match identity(session, cpu.wide(Slot::Base)) {
        Ok(pair) => pair,
        Err(c) => {
            fail(cpu, session, c);
            return;
        }
    };
    let start = u32::from(cpu.wide(Slot::Count)) << 16 | u32::from(cpu.wide(Slot::Data));
    let len = u32::from(cpu.wide(Slot::Source)) << 16 | u32::from(cpu.wide(Slot::Dest));
    let mut locks = LOCKS.borrow();
    let error = match cpu.low(Slot::Accumulator) {
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
    };
    if let Some(c) = error {
        fail(cpu, session, c);
    } else {
        cpu.set_carry(false);
    }
}
