use super::{Member, State, Threads};
use crate::arch::user::AddressSpace;
use crate::posix::{
    io::ProcessContext,
    process::signals::{self, Action},
};
use core::task::{Context, Poll};
use dosrs_native_exec::Call;
use dosrs_native_posix::Process;

pub(super) fn exception(vector: usize) -> u8 {
    match vector {
        0 | 16 | 19 => 8,
        1 | 3 => 5,
        6 => 4,
        17 => 7,
        _ => 11,
    }
}
fn wire(a: Action) -> [u8; 32] {
    let mut out = [0; 32];
    out[0..8].copy_from_slice(&(a.handler as u64).to_le_bytes());
    out[8..16].copy_from_slice(&(a.restorer as u64).to_le_bytes());
    out[16..24].copy_from_slice(&a.mask.to_le_bytes());
    out[24..28].copy_from_slice(&a.flags.to_le_bytes());
    out
}
pub(super) fn action(
    image: &mut AddressSpace,
    pid: u32,
    num: usize,
    set: usize,
    old: usize,
) -> Result<usize, isize> {
    let signo = u8::try_from(num).map_err(|_| -22isize)?;
    if old != 0 {
        image.check(old, 32, true).map_err(|_| -14isize)?;
    }
    let new = if set == 0 {
        None
    } else {
        let mut bytes = [0; 32];
        image.read(set, &mut bytes).map_err(|_| -14isize)?;
        let pointer = |offset: usize| {
            usize::try_from(u64::from_le_bytes(
                bytes[offset..offset + 8].try_into().unwrap(),
            ))
            .map_err(|_| -14isize)
        };
        let a = Action {
            handler: pointer(0)?,
            restorer: pointer(8)?,
            mask: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            flags: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
        };
        if a.flags & !(0x1000_0000 | 0x4000_0000 | 0x8000_0000 | 0x0400_0000) != 0 {
            return Err(-95);
        }
        if a.handler > 1
            && (!signals::trampoline(image, a.handler) || !signals::trampoline(image, a.restorer))
        {
            return Err(-14);
        }
        Some(a)
    };
    let previous = signals::install(pid, signo, new)?;
    if old != 0 {
        image.write(old, &wire(previous)).map_err(|_| -14isize)?;
    }
    Ok(0)
}

impl Member {
    pub(super) fn interrupt(
        &mut self,
        image: &mut AddressSpace,
        n: u8,
        a: Action,
    ) -> Result<(), ()> {
        let poll_mask = self
            .poll_request
            .as_ref()
            .map(|_| self.registers.as_ref().unwrap().signal_mask);
        if self.timer.is_some() || self.poll_request.is_some() {
            self.interrupt_wait(image);
        }
        self.tty_read.reset(self.tid);
        self.lock_request = None;
        let regs = self.registers.as_mut().ok_or(())?;
        let resume = match self.state {
            State::Suspended(mask) => {
                regs.signal_mask = mask;
                regs.result(-4);
                State::Ready
            }
            State::Call(
                Call::Read | Call::Write | Call::Readv | Call::Writev | Call::Waitpid,
                _,
            ) if a.flags & 0x1000_0000 != 0 => self.state,
            State::Ready => State::Ready,
            _ => {
                regs.result(-4);
                State::Ready
            }
        };
        self.interrupted.try_reserve(1).map_err(|_| ())?;
        regs.start_handler(image, a.handler, a.restorer, n)
            .map_err(|_| ())?;
        if let Some(mask) = poll_mask {
            regs.signal_mask = mask;
        }
        let extra = if a.flags & 0x4000_0000 != 0 {
            0
        } else {
            signals::bit(n)
        };
        regs.signal_mask |= (extra | a.mask) & !(signals::bit(9) | signals::bit(19));
        self.interrupted.push(resume);
        self.state = State::Ready;
        Ok(())
    }

    pub(super) fn sigreturn(&mut self) -> bool {
        let Some(resume) = self.interrupted.pop() else {
            return false;
        };
        if !self.registers.as_mut().unwrap().end_handler() {
            return false;
        }
        self.state = resume;
        true
    }
}

impl Threads {
    pub(super) fn thread_signal(&mut self, pid: u32, tid: i32, sig: i32) -> Result<usize, isize> {
        if !(0..=64).contains(&sig) {
            return Err(-22);
        }
        let t = self
            .0
            .iter_mut()
            .find(|t| t.tid == tid as u32 && !matches!(t.state, State::Exited(_) | State::Joined))
            .ok_or(-3isize)?;
        if sig == 0 {
            return Ok(0);
        }
        if matches!(sig, 9 | 18 | 19) {
            return signals::kill(pid, pid as i32, sig);
        }
        let n = sig as u8;
        let a = signals::disposition(pid, n, false);
        if a.handler != 1 && !(a.handler == 0 && matches!(n, 17 | 23 | 28)) {
            t.pending_signals |= signals::bit(n);
        }
        Ok(0)
    }
    pub(super) fn deliver(
        &mut self,
        image: &mut AddressSpace,
        p: &mut ProcessContext,
        cx: &Context,
    ) -> Option<Poll<super::Outcome>> {
        if signals::observe(p.getpid(), cx) {
            return Some(Poll::Pending);
        }
        for thread in &mut self.0 {
            if matches!(thread.state, State::Exited(_) | State::Joined) {
                continue;
            }
            let mask = thread.registers.as_ref()?.signal_mask;
            let directed = thread.pending_signals & !mask;
            let next = if directed != 0 {
                let n = (directed.trailing_zeros() + 1) as u8;
                thread.pending_signals &= !signals::bit(n);
                Some((n, signals::disposition(p.getpid(), n, true)))
            } else {
                signals::take(p.getpid(), mask)
            };
            let Some((n, a)) = next else { continue };
            match (n, a.handler) {
                (17 | 18 | 23 | 28, 0) => {}
                (19..=22, 0) => {
                    signals::stop(p.getpid(), n);
                    return Some(Poll::Pending);
                }
                (_, 0) => {
                    p.wait_signal = n;
                    return Some(Poll::Ready(Ok(128 + n)));
                }
                (_, 1) => {}
                _ => {
                    if thread.interrupt(image, n, a).is_err() {
                        p.wait_signal = 11;
                        return Some(Poll::Ready(Ok(139)));
                    }
                }
            }
        }
        None
    }
}
