//! BSP timekeeping and deadline wakeups. Sleepers retain a cancellable registration.
use super::{Member, State};
use crate::{
    arch::{platform, user::AddressSpace},
    kernel_cell::KernelCell,
};
use alloc::vec::Vec;
use core::{
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};
use dosrs_native_exec::Call;
const BILLION: u64 = 1_000_000_000;
#[derive(Clone, Copy)]
struct Source {
    start: u64,
    hz: u64,
    epoch: u64,
}
static SOURCE: KernelCell<Option<Source>> = KernelCell::new(None);
fn source() -> Result<Source, isize> {
    let mut slot = SOURCE.borrow();
    if let Some(source) = *slot {
        return Ok(source);
    }
    // PIT channel 2 is no longer used for secondary-CPU startup. Calibrate
    // on the BSP, where all syscall time reads and deadline checks execute.
    let mut best = u64::MAX;
    for _ in 0..3 {
        let begin = platform::timestamp();
        if !platform::pit_wait(11932) {
            return Err(-5);
        }
        best = best.min(platform::timestamp().saturating_sub(begin));
    }
    let hz = (best as u128 * 1_193_182 / 11932) as u64;
    if hz == 0 {
        return Err(-5);
    }
    let epoch = crate::bios::rtc::now().as_secs() * BILLION;
    let src = Source {
        start: platform::timestamp(),
        hz,
        epoch,
    };
    *slot = Some(src);
    Ok(src)
}
impl Source {
    fn ticks_ns(self) -> u64 {
        ((platform::timestamp().saturating_sub(self.start) as u128 * BILLION as u128)
            / self.hz as u128)
            .min(u64::MAX as u128) as u64
    }
    fn now(self, id: usize) -> Result<u64, isize> {
        match id {
            0 if self.epoch != 0 => Ok(self.epoch.saturating_add(self.ticks_ns())),
            0 => Err(-5),
            1 => Ok(self.ticks_ns()),
            _ => Err(-22),
        }
    }
}
struct Alarm {
    tid: u32,
    deadline: u64,
    waker: Waker,
}
static ALARMS: KernelCell<Vec<Alarm>> = KernelCell::new(Vec::new());
static ARMED: AtomicBool = AtomicBool::new(false);
pub(super) struct Timer {
    tid: u32,
    deadline: u64,
    pub(super) remainder: usize,
}
impl Timer {
    pub(super) fn after(tid: u32, delta: u64, cx: &Context) -> Result<Self, isize> {
        let now = source()?.ticks_ns();
        Self::arm(tid, now.checked_add(delta).ok_or(-75isize)?, 0, cx)
    }
    fn arm(tid: u32, deadline: u64, remainder: usize, cx: &Context) -> Result<Self, isize> {
        let mut all = ALARMS.borrow();
        all.try_reserve(1).map_err(|_| -12isize)?;
        all.push(Alarm {
            tid,
            deadline,
            waker: cx.waker().clone(),
        });
        ARMED.store(true, Ordering::Release);
        Ok(Self {
            tid,
            deadline,
            remainder,
        })
    }
    pub(super) fn expired(&self) -> bool {
        source().is_ok_and(|s| s.ticks_ns() >= self.deadline)
    }
    pub(super) fn remaining(&self) -> u64 {
        source().map_or(0, |s| self.deadline.saturating_sub(s.ticks_ns()))
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        let mut all = ALARMS.borrow();
        all.retain(|a| a.tid != self.tid);
        ARMED.store(!all.is_empty(), Ordering::Release);
    }
}
pub(crate) fn tick() {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    let Ok(src) = source() else {
        return;
    };
    let now = src.ticks_ns();
    let mut all = ALARMS.borrow();
    all.retain(|a| {
        if a.deadline > now {
            return true;
        }
        a.waker.wake_by_ref();
        false
    });
    ARMED.store(!all.is_empty(), Ordering::Release);
}
pub(super) fn read_ns(image: &AddressSpace, at: usize) -> Result<u64, isize> {
    let mut buf = [0; 16];
    image.read(at, &mut buf).map_err(|_| -14isize)?;
    let sec = i64::from_le_bytes(buf[..8].try_into().unwrap());
    let ns = i64::from_le_bytes(buf[8..].try_into().unwrap());
    if sec < 0 || !(0..BILLION as i64).contains(&ns) {
        return Err(-22);
    }
    (sec as u64)
        .checked_mul(BILLION)
        .and_then(|s| s.checked_add(ns as u64))
        .ok_or(-75)
}
pub(super) fn write_ns(image: &mut AddressSpace, at: usize, ns: u64) -> Result<(), isize> {
    let mut out = [0; 16];
    out[..8].copy_from_slice(&(ns / BILLION).to_le_bytes());
    out[8..].copy_from_slice(&(ns % BILLION).to_le_bytes());
    image.write(at, &out).map_err(|_| -14)
}
pub(super) fn call(
    t: &mut Member,
    image: &mut AddressSpace,
    call: Call,
    [a, b, c]: [usize; 3],
    cx: &Context,
) -> Poll<Result<usize, isize>> {
    let src = source()?;
    if call == Call::Clockread {
        let value = src.now(a)?;
        if c > 1 {
            return Poll::Ready(Err(-22));
        }
        if b != 0 || c == 0 {
            write_ns(
                image,
                b,
                if c == 0 {
                    value
                } else {
                    BILLION.div_ceil(src.hz)
                },
            )?;
        }
        return Poll::Ready(Ok(0));
    }
    let mono = src.ticks_ns();
    let (delta, rem, state) = if call == Call::Futextimed {
        if a & 3 != 0 {
            return Poll::Ready(Err(-22));
        }
        let mut word = [0; 4];
        image.read(a, &mut word).map_err(|_| -14isize)?;
        if u32::from_le_bytes(word) != b as u32 {
            return Poll::Ready(Err(-11));
        }
        (read_ns(image, c)?, 0, State::Futex(a))
    } else {
        let id = a & 0xff;
        let now = src.now(id)?;
        if a & !0x1ff != 0 {
            return Poll::Ready(Err(-22));
        }
        let ns = read_ns(image, b)?;
        let absolute = a & 0x100 != 0;
        let rem = if absolute { 0 } else { c };
        if rem != 0 {
            image.check(rem, 16, true).map_err(|_| -14isize)?;
        }
        (
            if absolute { ns.saturating_sub(now) } else { ns },
            rem,
            State::Sleep,
        )
    };
    if delta == 0 {
        return Poll::Ready(if call == Call::Futextimed {
            Err(-110)
        } else {
            Ok(0)
        });
    }
    t.timer = Some(Timer::arm(
        t.tid,
        mono.checked_add(delta).ok_or(-75isize)?,
        rem,
        cx,
    )?);
    t.state = state;
    Poll::Pending
}
