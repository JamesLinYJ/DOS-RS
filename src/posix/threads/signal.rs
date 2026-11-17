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
        self.signal_wait = None;
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
    pub(super) fn thread_signal(
        &mut self,
        pid: u32,
        uid: u32,
        tid: i32,
        sig: i32,
    ) -> Result<usize, isize> {
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
        if a.handler != 1 {
            t.pending_signals.add(
                n,
                signals::Origin {
                    code: -6,
                    pid,
                    uid,
                    status: 0,
                },
            )?;
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
            let next = if let Some((n, _)) = thread.pending_signals.take(!mask) {
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

pub(super) struct Wait {
    set: u64,
    info: usize,
}
pub(super) fn wait(
    t: &mut Member,
    image: &mut AddressSpace,
    pid: u32,
    [set, info, timeout]: [usize; 3],
    cx: &Context,
) -> Poll<Result<usize, isize>> {
    use super::clock;
    if t.signal_wait.is_none() {
        let mut bytes = [0; 8];
        image.read(set, &mut bytes).map_err(|_| -14isize)?;
        t.signal_wait = Some(Wait {
            set: u64::from_le_bytes(bytes) & !(signals::bit(9) | signals::bit(19)),
            info,
        });
    }
    let request = t.signal_wait.as_ref().unwrap();
    if request.info != 0 {
        image.check(request.info, 20, true).map_err(|_| -14isize)?;
    }
    if let Some((n, origin)) = t
        .pending_signals
        .take(request.set)
        .or_else(|| signals::take_waiting(pid, request.set))
    {
        if request.info != 0 {
            let fields = [
                u32::from(n),
                origin.code as u32,
                origin.pid,
                origin.uid,
                origin.status as u32,
            ];
            let mut bytes = [0; 20];
            for (slot, v) in bytes.chunks_exact_mut(4).zip(fields) {
                slot.copy_from_slice(&v.to_le_bytes());
            }
            image.write(request.info, &bytes).map_err(|_| -14isize)?;
        }
        return Poll::Ready(Ok(usize::from(n)));
    }
    if timeout != 0 && t.timer.is_none() {
        let interval = clock::read_ns(image, timeout)?;
        if interval == 0 {
            return Poll::Ready(Err(-11));
        }
        t.timer = Some(clock::Timer::after(t.tid, interval, cx)?);
    }
    if t.timer.as_ref().is_some_and(clock::Timer::expired) {
        return Poll::Ready(Err(-11));
    }
    Poll::Pending
}
