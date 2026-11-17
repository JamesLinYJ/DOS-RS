//! Process-directed standard signals coalesce until an eligible thread accepts them.
use super::{Record, TABLE};
#[path = "pending.rs"]
mod pending;
use alloc::vec::Vec;
use core::task::{Context, Waker};
pub(in crate::posix) use pending::{Origin, Pending};

#[derive(Clone, Copy, Default)]
pub(crate) struct Action {
    pub handler: usize,
    pub restorer: usize,
    pub mask: u64,
    pub flags: u32,
}
#[derive(Default)]
pub(super) struct Signals {
    actions: Vec<(u8, Action)>,
    pending: Pending,
    pub stopped: bool,
    pub transition: Option<i32>,
    owner: Option<Waker>,
}
pub fn bit(n: u8) -> u64 {
    1 << (n - 1)
}
pub fn trampoline(image: &crate::arch::user::AddressSpace, addr: usize) -> bool {
    let page = addr / dosrs_native_exec::PAGE * dosrs_native_exec::PAGE;
    image.pages().any(|(base, m)| base == page && m.executable)
}
impl Signals {
    pub(super) fn inherit(&self) -> Result<Self, isize> {
        let mut s = Self::default();
        s.actions
            .try_reserve_exact(self.actions.len())
            .map_err(|_| -12isize)?;
        s.actions.extend_from_slice(&self.actions);
        Ok(s)
    }
    pub(super) fn exec(&mut self) {
        self.actions.retain(|(_, a)| a.handler == 1);
    }
    fn action(&self, n: u8) -> Action {
        self.actions
            .iter()
            .find(|(s, _)| *s == n)
            .map_or(Action::default(), |(_, a)| *a)
    }
    pub(super) fn raise(&mut self, n: u8, origin: Origin) -> Result<(), isize> {
        if n == 18 {
            self.pending.clear(bit(19) | bit(20) | bit(21) | bit(22));
            if self.stopped {
                self.stopped = false;
                self.transition = Some(0xffff);
            }
        } else if matches!(n, 19..=22) {
            self.pending.clear(bit(18));
        }
        let h = self.action(n).handler;
        if h != 1 {
            self.pending.add(n, origin)?;
        }
        if let Some(w) = &self.owner {
            w.wake_by_ref();
        }
        Ok(())
    }
}

pub fn install(pid: u32, n: u8, action: Option<Action>) -> Result<Action, isize> {
    if !(1..=64).contains(&n) || matches!(n, 9 | 19) {
        return Err(-22);
    }
    let mut table = TABLE.borrow();
    let s = &mut table
        .iter_mut()
        .find(|p| p.pid == pid)
        .ok_or(-3isize)?
        .signals;
    let old = s.action(n);
    if let Some(a) = action {
        let existing = s.actions.iter().position(|(key, _)| *key == n);
        let handled = a.handler != 0 || a.flags != 0 || a.mask != 0;
        match (existing, handled) {
            (Some(i), true) => s.actions[i].1 = a,
            (Some(i), false) => {
                s.actions.swap_remove(i);
            }
            (None, true) => {
                s.actions.try_reserve(1).map_err(|_| -12isize)?;
                s.actions.push((n, a));
            }
            (None, false) => {}
        }
        if a.handler == 1 || a.handler == 0 && matches!(n, 17 | 18 | 23 | 28) {
            s.pending.clear(bit(n));
        }
    }
    Ok(old)
}
pub fn disposition(pid: u32, n: u8, dispatch: bool) -> Action {
    let mut table = TABLE.borrow();
    let Some(p) = table.iter_mut().find(|p| p.pid == pid) else {
        return Action::default();
    };
    let a = p.signals.action(n);
    if dispatch && a.flags & 0x8000_0000 != 0 {
        p.signals.actions.retain(|(s, _)| *s != n);
    }
    a
}
pub fn pending(pid: u32) -> u64 {
    TABLE
        .borrow()
        .iter()
        .find(|r| r.pid == pid)
        .map_or(0, |r| r.signals.pending.mask())
}
pub fn observe(pid: u32, cx: &Context) -> bool {
    let mut table = TABLE.borrow();
    let Some(r) = table.iter_mut().find(|r| r.pid == pid) else {
        return false;
    };
    if r.signals
        .owner
        .as_ref()
        .is_none_or(|w| !w.will_wake(cx.waker()))
    {
        r.signals.owner = Some(cx.waker().clone());
    }
    r.signals.stopped && r.signals.pending.mask() & bit(9) == 0
}
pub fn take(pid: u32, mask: u64) -> Option<(u8, Action)> {
    let mut table = TABLE.borrow();
    let r = table.iter_mut().find(|r| r.pid == pid)?;
    let (n, _) = r.signals.pending.take(!mask)?;
    let a = r.signals.action(n);
    if a.flags & 0x8000_0000 != 0 {
        r.signals.actions.retain(|(s, _)| *s != n);
    }
    Some((n, a))
}
pub fn take_waiting(pid: u32, set: u64) -> Option<(u8, Origin)> {
    TABLE
        .borrow()
        .iter_mut()
        .find(|p| p.pid == pid)?
        .signals
        .pending
        .take(set)
}
fn notify_parent(table: &mut [Record], pid: u32, origin: Origin) {
    if let Some(p) = table.iter_mut().find(|r| r.pid == pid) {
        p.waiting.wake();
        let _ = p.signals.raise(17, origin);
    }
}
pub fn stop(pid: u32, sig: u8) {
    let mut table = TABLE.borrow();
    if let Some(p) = table.iter_mut().find(|p| p.pid == pid) {
        p.signals.stopped = true;
        p.signals.transition = Some((i32::from(sig) << 8) | 0x7f);
        let parent = p.parent;
        let origin = Origin {
            code: 5,
            pid,
            uid: p.uid,
            status: i32::from(sig),
        };
        notify_parent(&mut table, parent, origin);
    }
}
pub fn kill(caller: u32, target: i32, sig: i32) -> Result<usize, isize> {
    if !(0..=64).contains(&sig) {
        return Err(-22);
    }
    let mut table = TABLE.borrow();
    let me = table.iter().find(|p| p.pid == caller).ok_or(-3isize)?;
    let (group, uid, session) = (me.group, me.uid, me.session);
    let mut matched = false;
    let mut permitted = false;
    for i in 0..table.len() {
        let p = &mut table[i];
        let selected = match target {
            0 => p.group == group,
            -1 => p.pid != caller,
            n if n > 0 => p.pid == n as u32,
            n => p.group == n.unsigned_abs(),
        };
        if !selected {
            continue;
        }
        matched = true;
        if uid != 0 && p.uid != uid && !(sig == 18 && p.session == session) {
            continue;
        }
        permitted = true;
        if sig != 0 && p.status.is_none() {
            let stopped = p.signals.stopped;
            p.signals.raise(
                sig as u8,
                Origin {
                    code: 0,
                    pid: caller,
                    uid,
                    status: 0,
                },
            )?;
            let (parent, continued) = (p.parent, stopped && !p.signals.stopped);
            if continued {
                let origin = Origin {
                    code: 6,
                    pid: p.pid,
                    uid: p.uid,
                    status: 18,
                };
                notify_parent(&mut table, parent, origin);
            }
        }
    }
    if permitted {
        Ok(0)
    } else if matched {
        Err(-1)
    } else {
        Err(-3)
    }
}

pub fn terminal_input(group: u32, sig: u8) {
    for p in TABLE
        .borrow()
        .iter_mut()
        .filter(|p| p.group == group && p.status.is_none())
    {
        let _ = p.signals.raise(sig, Origin::KERNEL);
    }
}
