//! User TLS, thread lifecycle and anonymous protections used by static libc.
use super::{Member, State, Threads, allocate_id};
use crate::arch::user::{AddressSpace, Thread};
use dosrs_native_exec::{Call, PAGE};

impl Threads {
    pub(super) fn musl_call(
        &mut self,
        i: usize,
        image: &mut AddressSpace,
        call: Call,
        [a, b, c]: [usize; 3],
    ) -> Result<usize, isize> {
        let caller = &mut self.0[i];
        match call {
            Call::Settls => {
                if a != 0 {
                    image
                        .check(a, size_of::<usize>(), true)
                        .map_err(|_| -14isize)?;
                }
                caller.registers.as_mut().unwrap().set_tls(a);
                Ok(0)
            }
            Call::Settidx => {
                if a != 0 {
                    image.check(a, 4, true).map_err(|_| -14isize)?;
                }
                caller.clear_tid = a;
                Ok(caller.tid as usize)
            }
            Call::Pthreadspawn => self.spawn_pthread(i, image, a, b, c),
            Call::Futexmove => {
                if a & 3 != 0 || b & 3 != 0 || a == b {
                    return Err(-22);
                }
                image.check(a, 4, false).map_err(|_| -14isize)?;
                image.check(b, 4, false).map_err(|_| -14isize)?;
                let mut buf = [0; 8];
                image.read(c, &mut buf).map_err(|_| -14isize)?;
                let wake = u32::from_ne_bytes(buf[..4].try_into().unwrap()) as usize;
                let mov = u32::from_ne_bytes(buf[4..].try_into().unwrap()) as usize;
                let mut changed = 0;
                for t in &mut self.0 {
                    if !matches!(t.state,State::Futex(addr) if addr==a) {
                        continue;
                    }
                    if changed < wake {
                        t.reply(0);
                    } else if changed - wake < mov {
                        t.state = State::Futex(b);
                    } else {
                        break;
                    }
                    changed += 1;
                }
                Ok(changed)
            }
            Call::Sigmask => {
                let thread = caller.registers.as_mut().unwrap();
                let old = thread.signal_mask;
                let mut new = old;
                if b != 0 {
                    let mut bytes = [0; 8];
                    image.read(b, &mut bytes).map_err(|_| -14isize)?;
                    let bits = u64::from_le_bytes(bytes) & !(1 << 8 | 1 << 18);
                    new = match a {
                        0 => old | bits,
                        1 => old & !bits,
                        2 => bits,
                        _ => return Err(-22),
                    };
                }
                if c != 0 {
                    image.write(c, &old.to_le_bytes()).map_err(|_| -14isize)?;
                }
                thread.signal_mask = new;
                Ok(0)
            }
            Call::Mapanon | Call::Protectmem => {
                let prot = if call == Call::Mapanon { b } else { c };
                if !matches!(prot, 0 | 1 | 2 | 3 | 4 | 5) {
                    return Err(-22);
                }
                let result = if call == Call::Mapanon {
                    image.map_protected(a, prot as u8)
                } else {
                    image.protect(a, b, prot as u8).map(|_| 0)
                };
                result.map_err(|e| {
                    if matches!(e, "invalid anonymous mapping" | "unmapped user address") {
                        -22
                    } else {
                        -12
                    }
                })
            }
            _ => Err(-38),
        }
    }

    fn spawn_pthread(
        &mut self,
        i: usize,
        image: &mut AddressSpace,
        entry: usize,
        arg: usize,
        params: usize,
    ) -> Result<usize, isize> {
        let mut words = [0usize; 5];
        for (n, value) in words.iter_mut().enumerate() {
            let mut b = [0; size_of::<usize>()];
            image
                .read(params.checked_add(n * b.len()).ok_or(-14isize)?, &mut b)
                .map_err(|_| -14isize)?;
            *value = usize::from_le_bytes(b);
        }
        let [base, top, tls, parent_tid, clear_tid] = words;
        if top.checked_sub(base).is_none_or(|len| len < PAGE - 128)
            || self.0.iter().any(|t| {
                !matches!(t.state, State::Joined) && base < t.stack.end && t.stack.start < top
            })
        {
            return Err(-22);
        }
        image.check(base, top - base, true).map_err(|_| -14isize)?;
        image.check(parent_tid, 4, true).map_err(|_| -14isize)?;
        image.check(clear_tid, 4, true).map_err(|_| -14isize)?;
        image
            .check(tls, size_of::<usize>(), true)
            .map_err(|_| -14isize)?;
        if !image
            .pages()
            .any(|(va, m)| va == entry / PAGE * PAGE && m.executable)
        {
            return Err(-14);
        }
        let sp = (top & !15) - if cfg!(target_arch = "x86") { 20 } else { 8 };
        let mut cpu = Thread::try_new(entry, sp, arg, 0).map_err(|_| -12isize)?;
        cpu.set_tls(tls);
        cpu.signal_mask = self.0[i].registers.as_ref().unwrap().signal_mask;
        self.0.try_reserve(1).map_err(|_| -12isize)?;
        let tid = allocate_id().map_err(|_| -11isize)?;
        let mut child = Member::new(tid, base..top, 0, cpu).map_err(|_| -12isize)?;
        child.clear_tid = clear_tid;
        image
            .write(sp, &0usize.to_le_bytes())
            .map_err(|_| -14isize)?;
        #[cfg(target_arch = "x86")]
        image
            .write(sp + 4, &arg.to_le_bytes())
            .map_err(|_| -14isize)?;
        image
            .write(parent_tid, &tid.to_le_bytes())
            .map_err(|_| -14isize)?;
        self.0.push(child);
        Ok(tid as usize)
    }

    pub(super) fn finish_pthread(
        &mut self,
        i: usize,
        image: &mut AddressSpace,
        base: usize,
        pages: usize,
    ) -> Result<(), isize> {
        if base != 0 || pages != 0 {
            image.unmap_anonymous(base, pages).map_err(|_| -22isize)?;
        }
        let clear = self.0[i].clear_tid;
        if clear != 0 && image.write(clear, &0u32.to_le_bytes()).is_ok() {
            if let Some(t) = self
                .0
                .iter_mut()
                .find(|t| matches!(t.state,State::Futex(addr) if addr==clear))
            {
                t.reply(0);
            }
        }
        self.0[i].registers = None;
        self.0[i].secondary = None;
        self.0[i].state = State::Joined;
        Ok(())
    }
}

pub(super) fn vectors(
    image: &mut AddressSpace,
    p: &super::ProcessContext,
    cx: &core::task::Context,
    call: Call,
    [fd, vector, count]: [usize; 3],
    tty_read: &mut crate::posix::io::ReadClock,
) -> core::task::Poll<Result<usize, isize>> {
    use alloc::vec::Vec;
    use core::task::{Poll, ready};
    if count > 1024 {
        return Poll::Ready(Err(-22));
    }
    let reading = call == Call::Readv;
    let mut areas = Vec::new();
    areas.try_reserve_exact(count).map_err(|_| -12isize)?;
    let mut size = 0usize;
    for i in 0..count {
        let mut pair = [0; 2 * size_of::<usize>()];
        image
            .read(
                vector.checked_add(i * pair.len()).ok_or(-14isize)?,
                &mut pair,
            )
            .map_err(|_| -14isize)?;
        let (a, n) = pair.split_at(size_of::<usize>());
        let addr = usize::from_le_bytes(a.try_into().unwrap());
        let len = usize::from_le_bytes(n.try_into().unwrap());
        size = size
            .checked_add(len)
            .filter(|n| *n <= isize::MAX as usize)
            .ok_or(-22isize)?;
        image.check(addr, len, reading).map_err(|_| -14isize)?;
        areas.push((addr, len));
    }
    // Transfer one kernel I/O block atomically, even when multiple iovecs
    // contribute to a single PIPE_BUF-sized write.
    if !reading {
        p.check_record_size(fd, size)?;
    }
    let mut bytes = [0; 4096];
    let buf = &mut bytes[..size.min(4096)];
    let transfer = if reading {
        ready!(p.poll_read(fd, cx, buf, tty_read)).map_err(super::super::executable::errno)?
    } else {
        buf.len()
    };
    let mut at = 0usize;
    for (addr, len) in areas {
        let end = transfer.min(at.saturating_add(len));
        if reading {
            image.write(addr, &buf[at..end])
        } else {
            image.read(addr, &mut buf[at..end])
        }
        .map_err(|_| -14isize)?;
        at = end;
        if at == transfer {
            break;
        }
    }
    if reading {
        Poll::Ready(Ok(transfer))
    } else {
        p.poll_write(fd, cx, buf)
            .map(|r| r.map_err(super::super::executable::errno))
    }
}
