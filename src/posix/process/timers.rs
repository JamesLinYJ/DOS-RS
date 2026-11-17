//! Process-owned POSIX timers. Fork starts with no timers and exec removes them.
use super::{
    TABLE,
    signals::{Origin, Signals, TimerInfo},
};
use crate::{arch::user::AddressSpace, posix::threads::clock};
use alloc::vec::Vec;
use core::task::Context;
use dosrs_native_exec::Call;

struct Entry {
    id: u32,
    clock: usize,
    signal: Option<u8>,
    value: u64,
    interval: u64,
    alarm: Option<clock::Timer>,
    overruns: u32,
    last_overrun: u32,
}

#[derive(Default)]
pub(super) struct Timers {
    next: u32,
    entries: Vec<Entry>,
}

impl Timers {
    pub(super) fn clear(&mut self, signals: &mut Signals) {
        for entry in &self.entries {
            signals.pending.clear_timer(entry.id);
        }
        self.entries.clear();
    }

    pub(super) fn delivered(&mut self, origin: &mut Origin) {
        if let Some(info) = &mut origin.timer
            && let Some(entry) = self.entries.iter_mut().find(|e| e.id == info.id)
        {
            entry.last_overrun = entry.overruns;
            info.overruns = entry.overruns;
            entry.overruns = 0;
        }
    }

    pub(super) fn poll(&mut self, signals: &mut Signals, cx: &Context) {
        for entry in &mut self.entries {
            let Some(alarm) = &mut entry.alarm else {
                continue;
            };
            if !alarm.expired() {
                continue;
            }
            let late = alarm.overdue();
            let missed = if entry.interval == 0 {
                0
            } else {
                late / entry.interval
            };
            if let Some(n) = entry.signal {
                let additional =
                    missed.saturating_add(u64::from(signals.pending.has_timer(entry.id)));
                entry.overruns = if signals.pending.has_timer(entry.id) {
                    entry
                        .overruns
                        .saturating_add(additional.min(i32::MAX as u64) as u32)
                } else {
                    missed.min(i32::MAX as u64) as u32
                }
                .min(i32::MAX as u32);
                let _ = signals.raise(
                    n,
                    Origin {
                        code: -2,
                        timer: Some(TimerInfo {
                            id: entry.id,
                            overruns: 0,
                            value: entry.value,
                        }),
                        ..Origin::default()
                    },
                );
            }
            let next = missed
                .checked_add(1)
                .and_then(|steps| steps.checked_mul(entry.interval));
            if entry.interval == 0 || next.is_none_or(|delta| alarm.advance(delta, cx).is_err()) {
                entry.alarm = None;
            }
        }
    }
}

fn write_spec(image: &mut AddressSpace, at: usize, entry: &Entry) -> Result<(), isize> {
    clock::write_ns(image, at, entry.interval)?;
    clock::write_ns(
        image,
        at + 16,
        entry.alarm.as_ref().map_or(0, clock::Timer::remaining),
    )
}

pub(in crate::posix) fn call(
    image: &mut AddressSpace,
    pid: u32,
    call: Call,
    [a, b, c]: [usize; 3],
    cx: &Context,
) -> Result<usize, isize> {
    let mut table = TABLE.borrow();
    let process = table.iter_mut().find(|p| p.pid == pid).ok_or(-3isize)?;
    let timers = &mut process.timers;
    timers.poll(&mut process.signals, cx);
    if call == Call::Timercreate {
        clock::now(a)?;
        if timers.entries.len() >= 128 {
            return Err(-11);
        }
        image.check(c, 4, true).map_err(|_| -14isize)?;
        let id = timers
            .next
            .checked_add(1)
            .filter(|id| *id <= i32::MAX as u32)
            .ok_or(-11isize)?;
        let (signal, value) = if b == 0 {
            (Some(14), u64::from(id))
        } else {
            let mut wire = [0; 16];
            image.read(b, &mut wire).map_err(|_| -14isize)?;
            let notify = u32::from_le_bytes(wire[..4].try_into().unwrap());
            let signo = u32::from_le_bytes(wire[4..8].try_into().unwrap());
            let signal = match notify {
                0 if (1..=64).contains(&signo) => Some(signo as u8),
                0 => return Err(-22),
                1 => None,
                2 | 4 => return Err(-95),
                _ => return Err(-22),
            };
            (signal, u64::from_le_bytes(wire[8..].try_into().unwrap()))
        };
        timers.entries.try_reserve(1).map_err(|_| -11isize)?;
        process.signals.pending.reserve_timers()?;
        image.write(c, &id.to_le_bytes()).map_err(|_| -14isize)?;
        timers.entries.push(Entry {
            id,
            clock: a,
            signal,
            value,
            interval: 0,
            alarm: None,
            overruns: 0,
            last_overrun: 0,
        });
        timers.next = id;
        return Ok(0);
    }
    let id = u32::try_from(a).map_err(|_| -22isize)?;
    let index = timers
        .entries
        .iter()
        .position(|e| e.id == id)
        .ok_or(-22isize)?;
    if call == Call::Timerdelete {
        process.signals.pending.clear_timer(id);
        timers.entries.remove(index);
        return Ok(0);
    }
    let entry = &mut timers.entries[index];
    if call == Call::Timeroverrun {
        return Ok(entry.last_overrun as usize);
    }
    if call == Call::Timerget {
        image.check(b, 32, true).map_err(|_| -14isize)?;
        write_spec(image, b, entry)?;
        return Ok(0);
    }
    if b & !1 != 0 {
        return Err(-22);
    }
    let mut pointers = [0; 2 * size_of::<usize>()];
    image.read(c, &mut pointers).map_err(|_| -14isize)?;
    let size = size_of::<usize>();
    let new = usize::from_le_bytes(pointers[..size].try_into().unwrap());
    let old = usize::from_le_bytes(pointers[size..].try_into().unwrap());
    image.check(new, 32, false).map_err(|_| -14isize)?;
    let interval = clock::read_ns(image, new)?;
    let value = clock::read_ns(image, new + 16)?;
    if old != 0 {
        image.check(old, 32, true).map_err(|_| -14isize)?;
    }
    let alarm = if value == 0 {
        None
    } else {
        let delta = if b == 1 {
            value.saturating_sub(clock::now(entry.clock)?)
        } else {
            value
        };
        Some(clock::Timer::after(pid, delta, cx)?)
    };
    if old != 0 {
        write_spec(image, old, entry)?;
    }
    entry.interval = interval;
    entry.alarm = alarm;
    if !process.signals.pending.has_timer(id) {
        entry.overruns = 0;
    }
    cx.waker().wake_by_ref();
    Ok(0)
}
