//! Process relationships live past exit until the parent collects the wait status.
//! Ready children enter their terminal's group between scheduler rounds.
pub(super) mod limits;
pub(super) mod signals;
pub(super) mod timers;
use super::{Waiters, executable::Executable, io::ProcessContext};
use crate::kernel_cell::KernelCell;
use alloc::vec::Vec;
use core::task::{Context, Poll};

pub(super) type NewProcesses = KernelCell<Vec<(Executable, ProcessContext)>>;

struct Record {
    pid: u32,
    uid: u32,
    signals: signals::Signals,
    timers: timers::Timers,
    limits: limits::Limits,
    parent: u32,
    group: u32,
    session: u32,
    execed: bool,
    status: Option<i32>,
    waiting: Waiters,
}

static TABLE: KernelCell<Vec<Record>> = KernelCell::new(Vec::new());

pub fn register(pid: u32, parent: u32, uid: u32) -> Result<(), isize> {
    let mut records = TABLE.borrow();
    let (group, session) = records
        .iter()
        .find(|p| p.pid == parent)
        .map_or((pid, pid), |p| (p.group, p.session));
    let signals = records
        .iter()
        .find(|p| p.pid == parent)
        .map(|p| p.signals.inherit())
        .transpose()?
        .unwrap_or_default();
    let limits = records
        .iter()
        .find(|p| p.pid == parent)
        .map_or_else(limits::Limits::default, |p| p.limits);
    records.try_reserve(1).map_err(|_| -12isize)?;
    records.push(Record {
        pid,
        uid,
        signals,
        timers: timers::Timers::default(),
        limits,
        parent,
        group,
        session,
        execed: false,
        status: None,
        waiting: Waiters::default(),
    });
    Ok(())
}

pub(super) fn set_uid(pid: u32, uid: u32) -> Result<(), isize> {
    let mut table = TABLE.borrow();
    let record = table.iter_mut().find(|r| r.pid == pid).ok_or(-3isize)?;
    record.uid = uid;
    Ok(())
}

pub fn exec(pid: u32) {
    if let Some(caller) = TABLE.borrow().iter_mut().find(|p| p.pid == pid) {
        caller.execed = true;
        caller.signals.exec();
        caller.timers.clear(&mut caller.signals);
    }
}

pub fn exited(pid: u32, status: i32) {
    let mut table = TABLE.borrow();
    if let Some(me) = table.iter_mut().find(|p| p.pid == pid) {
        me.status = Some(status);
        me.timers.clear(&mut me.signals);
        let parent = me.parent;
        let uid = me.uid;
        if let Some(parent) = table.iter_mut().find(|p| p.pid == parent) {
            parent.waiting.wake();
            let signaled = status & 127 != 0;
            let _ = parent.signals.raise(
                17,
                signals::Origin {
                    code: if signaled { 2 } else { 1 },
                    pid,
                    uid,
                    status: if signaled {
                        status & 127
                    } else {
                        (status >> 8) & 255
                    },
                    timer: None,
                },
            );
        }
    }
    for p in table.iter_mut().filter(|p| p.parent == pid) {
        p.parent = 0;
    }
    table.retain(|p| p.parent != 0 || p.status.is_none());
}

pub(super) fn live_pid(pid: u32) -> bool {
    TABLE
        .borrow()
        .iter()
        .any(|p| p.pid == pid && p.status.is_none())
}

pub fn parent(pid: u32) -> u32 {
    TABLE
        .borrow()
        .iter()
        .find(|p| p.pid == pid)
        .map_or(0, |p| p.parent)
}

pub fn wait(me: u32, selector: i32, flags: usize, cx: &Context) -> Poll<Result<(u32, i32), isize>> {
    if flags & !(1 | 2 | 8) != 0 {
        return Poll::Ready(Err(-22));
    }
    let mut table = TABLE.borrow();
    let own = table.iter().position(|p| p.pid == me).ok_or(-3isize)?;
    let group = table[own].group;
    let matches = |p: &Record| {
        p.parent == me
            && match selector {
                -1 => true,
                0 => p.group == group,
                x if x > 0 => p.pid == x as u32,
                x => p.group == x.unsigned_abs(),
            }
    };
    if let Some(index) = table.iter().position(|p| matches(p) && p.status.is_some()) {
        let child = table.swap_remove(index);
        return Poll::Ready(Ok((child.pid, child.status.unwrap())));
    }
    if let Some(p) = table.iter_mut().find(|p| {
        matches(p)
            && p.signals.transition.is_some_and(|s| {
                if s == 0xffff {
                    flags & 8 != 0
                } else {
                    flags & 2 != 0
                }
            })
    }) {
        return Poll::Ready(Ok((p.pid, p.signals.transition.take().unwrap())));
    }
    if !table.iter().any(matches) {
        return Poll::Ready(Err(-10));
    }
    if flags & 1 != 0 {
        return Poll::Ready(Ok((0, 0)));
    }
    table[own].waiting.wait(cx)
}

pub fn group(me: u32, pid: i32, session: bool) -> Result<usize, isize> {
    let who = if pid == 0 { me } else { pid as u32 };
    let table = TABLE.borrow();
    let process = table.iter().find(|p| p.pid == who).ok_or(-3isize)?;
    Ok(if session {
        process.session
    } else {
        process.group
    } as usize)
}

pub fn new_session(me: u32) -> Result<usize, isize> {
    let mut table = TABLE.borrow();
    if table.iter().any(|p| p.group == me) {
        return Err(-1);
    }
    let process = table.iter_mut().find(|p| p.pid == me).ok_or(-3isize)?;
    process.session = me;
    process.group = me;
    Ok(me as usize)
}

pub fn set_group(me: u32, pid: i32, pgid: i32) -> Result<usize, isize> {
    if pgid < 0 {
        return Err(-22);
    }
    let who = if pid == 0 { me } else { pid as u32 };
    let dest = if pgid == 0 { who } else { pgid as u32 };
    let mut table = TABLE.borrow();
    let session = table.iter().find(|p| p.pid == me).ok_or(-3isize)?.session;
    let i = table
        .iter()
        .position(|p| p.pid == who && (who == me || p.parent == me))
        .ok_or(-3isize)?;
    if who != me && table[i].execed {
        return Err(-13);
    }
    if table[i].session != session
        || table[i].session == who
        || (who != dest
            && !table
                .iter()
                .any(|p| p.group == dest && p.session == session))
    {
        return Err(-1);
    }
    table[i].group = dest;
    Ok(0)
}

pub fn attach(pid: u32, session: u32, group: u32) {
    if let Some(p) = TABLE.borrow().iter_mut().find(|p| p.pid == pid) {
        p.session = session;
        p.group = group;
    }
}
pub fn group_in_session(group: u32, session: u32) -> bool {
    TABLE
        .borrow()
        .iter()
        .any(|p| p.group == group && p.session == session && p.status.is_none())
}
