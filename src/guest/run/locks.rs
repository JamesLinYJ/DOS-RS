//! DOS byte-range locks shared by all sessions on the mounted volume.
use super::{CpuState, Session, Slot, fail};
use crate::vfs::locks::{self, FileKey, Owner};

fn identity(session: &Session, h: u16) -> Result<(FileKey, Owner), u16> {
    let file = session.handles.get(h).ok_or(6u16)?;
    let object = file.object.as_ref().ok_or(1u16)?;
    Ok((
        FileKey(object.identity()),
        Owner::Guest {
            terminal: session.terminal,
            description: session.handles.slot(h).unwrap(),
        },
    ))
}

pub(super) fn discard_closed(session: &Session) {
    locks::discard(
        |owner| matches!(owner,Owner::Guest{terminal,description} if terminal==session.terminal && !session.handles.live(description)),
    );
}

pub(super) fn blocked(session: &Session, h: u16, start: u32, len: u32) -> bool {
    let Ok((file, owner)) = identity(session, h) else {
        return false;
    };
    locks::blocked(file, owner, start, len)
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
    let error = locks::change(file, owner, cpu.low(Slot::Accumulator), start, len);
    if let Some(c) = error {
        fail(cpu, session, c);
    } else {
        cpu.set_carry(false);
    }
}
