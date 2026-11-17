//! Process-owned threads. CPUs execute runnable threads in parallel. At a
//! quantum boundary, all CPUs release the process pages before the kernel
//! reads a syscall buffer, changes a mapping or tears down the process.
mod affinity;
pub(super) mod clock;
mod musl;
mod poll;
mod signal;
use super::process::signals;
use super::{
    executable::file_call,
    io::{ProcessContext, allocate_id},
};
use crate::{
    arch::user::{AddressSpace, Stop, Thread},
    tasks::user::{self, Completion},
};
use alloc::{format, string::String, sync::Arc, vec::Vec};
use core::{
    ops::Range,
    task::{Context, Poll},
};
use dosrs_native_exec::{Call, END, PAGE, STACK};
use dosrs_native_posix::Process;

#[derive(Clone, Copy)]
enum State {
    Ready,
    Running,
    Suspended(u64),
    Call(Call, [usize; 3]),
    Futex(usize),
    Sleep,
    Exited(u8),
    Joined,
}

struct Member {
    tid: u32,
    secondary: Option<affinity::ActiveTid>,
    cpu: usize,
    stack: Range<usize>,
    errno: usize,
    pending_signals: signals::Pending,
    signal_wait: Option<signal::Wait>,
    interrupted: Vec<State>,
    clear_tid: usize,
    tty_read: super::io::ReadClock,
    timer: Option<clock::Timer>,
    poll_request: Option<poll::Request>,
    lock_request: Option<super::io::record::Request>,
    registers: Option<Thread>,
    returned: Arc<Completion>,
    state: State,
}

impl Member {
    fn new(
        tid: u32,
        stack: Range<usize>,
        errno: usize,
        registers: Thread,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            tid,
            secondary: if tid == 0 {
                None
            } else {
                Some(affinity::ActiveTid::register(tid)?)
            },
            cpu: user::assign_cpu(),
            stack,
            errno,
            pending_signals: signals::Pending::default(),
            signal_wait: None,
            interrupted: Vec::new(),
            clear_tid: 0,
            tty_read: super::io::ReadClock::new(tid),
            timer: None,
            poll_request: None,
            lock_request: None,
            registers: Some(registers),
            returned: Completion::new()?,
            state: State::Ready,
        })
    }
    fn interrupt_wait(&mut self, image: &mut AddressSpace) {
        if let Some(timer) = &self.timer
            && timer.remainder != 0
        {
            let _ = clock::write_ns(image, timer.remainder, timer.remaining());
        }
        self.reply(-4);
    }
    fn reply(&mut self, value: isize) {
        self.tty_read.reset(self.tid);
        self.signal_wait = None;
        self.lock_request = None;
        self.timer = None;
        if let Some(request) = self.poll_request.take() {
            request.restore(self);
        }
        self.registers
            .as_mut()
            .expect("stopped user thread")
            .result(value);
        self.state = State::Ready;
    }
}

pub(super) struct Threads(Vec<Member>);
type Outcome = Result<u8, (u8, String)>;

impl Threads {
    pub fn new(main: Thread) -> Result<Self, &'static str> {
        Self::from_caller(main, STACK..END, 0)
    }
    pub(super) fn from_caller(
        main: Thread,
        stack: Range<usize>,
        errno: usize,
    ) -> Result<Self, &'static str> {
        let mut threads = Vec::new();
        threads
            .try_reserve(1)
            .map_err(|_| "insufficient thread memory")?;
        threads.push(Member::new(0, stack, errno, main)?);
        Ok(Self(threads))
    }

    pub(super) fn inherit_handlers(&mut self, caller: &Threads, i: usize) -> Result<(), isize> {
        let states = &caller.0[i].interrupted;
        let saved = &mut self.0[0].interrupted;
        saved
            .try_reserve_exact(states.len())
            .map_err(|_| -12isize)?;
        saved.extend_from_slice(states);
        Ok(())
    }
    pub(super) fn set_mask(&mut self, mask: u64) {
        self.0[0].registers.as_mut().unwrap().signal_mask = mask;
    }
    fn spawn(
        &mut self,
        image: &mut AddressSpace,
        entry: usize,
        argument: usize,
        descriptor: usize,
    ) -> Result<usize, isize> {
        let mut words = [0usize; 3];
        for (i, word) in words.iter_mut().enumerate() {
            let mut bytes = [0; size_of::<usize>()];
            let address = descriptor.checked_add(i * bytes.len()).ok_or(-14isize)?;
            image.read(address, &mut bytes).map_err(|_| -14isize)?;
            *word = usize::from_le_bytes(bytes);
        }
        let [base, size, copy_length] = words;
        let end = base.checked_add(size).ok_or(-22isize)?;
        if base & 15 != 0 || size < PAGE || copy_length > PAGE || copy_length > size - 128 {
            return Err(-22);
        }
        if self
            .0
            .iter()
            .any(|t| !matches!(t.state, State::Joined) && base < t.stack.end && t.stack.start < end)
        {
            return Err(-22);
        }
        image.check(base, size, true).map_err(|_| -14isize)?;
        if !image
            .pages()
            .any(|(a, mapping)| a == entry / PAGE * PAGE && mapping.executable)
        {
            return Err(-14);
        }
        let mut copy = Vec::new();
        copy.try_reserve_exact(copy_length).map_err(|_| -12isize)?;
        copy.resize(copy_length, 0);
        image.read(argument, &mut copy).map_err(|_| -14isize)?;
        let arg = if copy.is_empty() { argument } else { base + 16 };
        let sp = (end & !15) - if cfg!(target_arch = "x86") { 20 } else { 8 };
        let cpu = Thread::try_new(entry, sp, arg, 0).map_err(|_| -12isize)?;
        self.0.try_reserve(1).map_err(|_| -12isize)?;
        let tid = allocate_id().map_err(|_| -11isize)?;
        let child = Member::new(tid, base..end, base, cpu).map_err(|_| -12isize)?;
        image
            .write(base, &0u32.to_le_bytes())
            .map_err(|_| -14isize)?;
        if !copy.is_empty() {
            image.write(arg, &copy).map_err(|_| -14isize)?;
        }
        image
            .write(sp, &0usize.to_le_bytes())
            .map_err(|_| -14isize)?;
        #[cfg(target_arch = "x86")]
        image
            .write(sp + 4, &arg.to_le_bytes())
            .map_err(|_| -14isize)?;
        self.0.push(child);
        Ok(tid as usize)
    }

    fn collect(&mut self, image: &mut AddressSpace, pid: u32) -> Result<(), (u8, String)> {
        #[cfg(feature = "thread-trace")]
        let trace = self.0.len() > 1;
        for t in &mut self.0 {
            if !matches!(t.state, State::Running) {
                continue;
            }
            let returned = t
                .returned
                .take()
                .expect("all dispatched threads have returned");
            #[cfg(feature = "thread-trace")]
            if trace {
                let (slot, start, end) = returned.interval;
                for byte in format!(
                    "THREAD pid={pid} tid={} cpu={slot} start={start} end={end}\r\n",
                    t.tid
                )
                .bytes()
                {
                    crate::bios::pc::serial_write(0x3f8, byte);
                }
            }
            t.registers = Some(returned.thread);
            match returned
                .outcome
                .map_err(|message| (126, format!("{message}\n")))?
            {
                Stop::Fault(vector) => {
                    let signo = signal::exception(vector);
                    let action = signals::disposition(pid, signo, true);
                    if action.handler > 1
                        && t.registers.as_ref().unwrap().signal_mask & signals::bit(signo) == 0
                    {
                        t.state = State::Ready;
                        if t.interrupt(image, signo, action).is_ok() {
                            continue;
                        }
                    }
                    return Err((
                        128 + vector.min(127) as u8,
                        format!("native process {pid}: user exception {vector:02x}\n"),
                    ));
                }
                Stop::Slice => t.state = State::Ready,
                Stop::Call(number) => match Call::decode(number) {
                    None => t.reply(-38),
                    Some((call, count)) => match t.registers.as_ref().unwrap().args(image, count) {
                        Some(args) => t.state = State::Call(call, args),
                        None => t.reply(-14),
                    },
                },
            }
        }
        Ok(())
    }

    fn join(&mut self, caller: usize, target: usize) -> Poll<Result<usize, isize>> {
        if target == self.0[caller].tid as usize {
            return Poll::Ready(Err(-35));
        }
        let Some(child) = self.0.iter_mut().find(|t| t.tid as usize == target) else {
            return Poll::Ready(Err(-3));
        };
        match child.state {
            State::Exited(code) => {
                child.state = State::Joined;
                Poll::Ready(Ok(code as usize))
            }
            State::Joined => Poll::Ready(Err(-3)),
            _ => Poll::Pending,
        }
    }

    fn futex(
        &mut self,
        i: usize,
        _pid: u32,
        image: &AddressSpace,
        call: Call,
        address: usize,
        value: usize,
    ) -> Poll<Result<usize, isize>> {
        if address & 3 != 0 {
            return Poll::Ready(Err(-22));
        }
        let mut bytes = [0; 4];
        image.read(address, &mut bytes).map_err(|_| -14isize)?;
        if call == Call::Futexwait {
            if u32::from_le_bytes(bytes) != value as u32 {
                return Poll::Ready(Err(-11));
            }
            self.0[i].state = State::Futex(address);
            return Poll::Pending;
        }
        let mut woke = 0;
        #[cfg(feature = "thread-trace")]
        let from = (self.0[i].tid, self.0[i].cpu);
        for t in &mut self.0 {
            if woke == value {
                break;
            }
            if matches!(t.state, State::Futex(key) if key == address) {
                t.reply(0);
                #[cfg(feature = "thread-trace")]
                for byte in format!(
                    "FUTEX pid={} from={} to={} from_cpu={} to_cpu={}\r\n",
                    _pid, from.0, t.tid, from.1, t.cpu
                )
                .bytes()
                {
                    crate::bios::pc::serial_write(0x3f8, byte);
                }
                woke += 1;
            }
        }
        Poll::Ready(Ok(woke))
    }

    pub fn poll(
        &mut self,
        image: &mut AddressSpace,
        p: &mut ProcessContext,
        cx: &Context,
    ) -> Poll<Outcome> {
        if self
            .0
            .iter()
            .any(|t| matches!(t.state, State::Running) && !t.returned.ready())
        {
            return Poll::Pending;
        }
        if self.0[0].tid == 0 {
            self.0[0].tid = p.getpid();
            self.0[0].tty_read.reset(p.getpid());
        }
        self.collect(image, p.getpid())?;
        for t in &mut self.0 {
            if matches!(t.state, State::Futex(_) | State::Sleep)
                && t.timer.as_ref().is_some_and(|timer| timer.expired())
            {
                t.reply(if matches!(t.state, State::Futex(_)) {
                    -110
                } else {
                    0
                });
            }
        }

        if signals::observe(p.getpid(), cx) {
            return Poll::Pending;
        }
        for i in 0..self.0.len() {
            let State::Call(call, args) = self.0[i].state else {
                continue;
            };
            let [a, b, c] = args;
            p.caller_mask = self.0[i].registers.as_ref().unwrap().signal_mask;
            let result = match call {
                Call::Exit => return Poll::Ready(Ok(a as u8)),
                Call::Fork => Poll::Ready((|| {
                    let new = p.children.upgrade().ok_or(-11isize)?;
                    let mut queue = new.borrow();
                    queue.try_reserve(1).map_err(|_| -12isize)?;
                    let caller = &self.0[i];
                    let mut child = super::executable::Executable::fork(
                        image,
                        caller.registers.as_ref().unwrap(),
                        caller.stack.clone(),
                        caller.errno,
                    )?;
                    child.inherit_handlers(self, i)?;
                    let context = p.inherit().map_err(|_| -11isize)?;
                    let pid = context.getpid();
                    queue.push((child, context));
                    Ok(pid as usize)
                })()),
                Call::Execve => match super::executable::execve(image, p, args) {
                    Ok(mut task) => {
                        task.set_mask(self.0[i].registers.as_ref().unwrap().signal_mask);
                        super::process::exec(p.getpid());
                        p.close_exec_fds();
                        task.replace(image, self);
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                    Err(error) => Poll::Ready(Err(error)),
                },
                Call::Waitpid => {
                    if b != 0 && image.check(b, 4, true).is_err() {
                        Poll::Ready(Err(-14))
                    } else {
                        super::process::wait(p.getpid(), a as i32, c, cx).map(|result| {
                            let (pid, status) = result?;
                            if pid != 0 && b != 0 {
                                image
                                    .write(b, &status.to_le_bytes())
                                    .map_err(|_| -14isize)?;
                            }
                            Ok(pid as usize)
                        })
                    }
                }
                Call::Getppid => Poll::Ready(Ok(super::process::parent(p.getpid()) as usize)),
                Call::Getsid | Call::Getpgid => Poll::Ready(super::process::group(
                    p.getpid(),
                    a as i32,
                    call == Call::Getsid,
                )),
                Call::Setsid => Poll::Ready(super::process::new_session(p.getpid())),
                Call::Setpgid => {
                    Poll::Ready(super::process::set_group(p.getpid(), a as i32, b as i32))
                }
                Call::Sigwait => signal::wait(&mut self.0[i], image, p.getpid(), args, cx),
                Call::Tkill => {
                    Poll::Ready(self.thread_signal(p.getpid(), p.uid, a as i32, b as i32))
                }
                Call::Kill => Poll::Ready(signals::kill(p.getpid(), a as i32, b as i32)),
                Call::Sigaction => Poll::Ready(signal::action(image, p.getpid(), a, b, c)),
                Call::Sigreturn => {
                    if self.0[i].sigreturn() {
                        cx.waker().wake_by_ref();
                        continue;
                    } else {
                        Poll::Ready(Err(-22))
                    }
                }
                Call::Sigpending => {
                    let mask = self.0[i].registers.as_ref().unwrap().signal_mask;
                    Poll::Ready(
                        image
                            .write(
                                a,
                                &((signals::pending(p.getpid())
                                    | self.0[i].pending_signals.mask())
                                    & mask)
                                    .to_le_bytes(),
                            )
                            .map(|_| 0)
                            .map_err(|_| -14),
                    )
                }
                Call::Sigsuspend => {
                    let mut bytes = [0; 8];
                    if image.read(a, &mut bytes).is_err() {
                        Poll::Ready(Err(-14))
                    } else {
                        let regs = self.0[i].registers.as_mut().unwrap();
                        let old = regs.signal_mask;
                        regs.signal_mask =
                            u64::from_le_bytes(bytes) & !(signals::bit(9) | signals::bit(19));
                        self.0[i].state = State::Suspended(old);
                        Poll::Pending
                    }
                }
                Call::Threxit => {
                    self.0[i].secondary = None;
                    self.0[i].state = State::Exited(a as u8);
                    self.0[i].registers = None;
                    cx.waker().wake_by_ref();
                    continue;
                }
                Call::Clockread | Call::Clocksleep | Call::Futextimed => {
                    clock::call(&mut self.0[i], image, call, args, cx)
                }
                Call::Poll | Call::Ppoll => poll::call(&mut self.0[i], image, p, call, args, cx),
                Call::Readv | Call::Writev => {
                    musl::vectors(image, p, cx, call, args, &mut self.0[i].tty_read)
                }
                Call::Threadfinish => match self.finish_pthread(i, image, a, b) {
                    Ok(()) => {
                        cx.waker().wake_by_ref();
                        continue;
                    }
                    Err(error) => Poll::Ready(Err(error)),
                },
                Call::Settls
                | Call::Settidx
                | Call::Pthreadspawn
                | Call::Sigmask
                | Call::Mapanon
                | Call::Protectmem
                | Call::Futexmove => Poll::Ready(self.musl_call(i, image, call, args)),
                Call::Thrspawn => Poll::Ready(self.spawn(image, a, b, c)),
                Call::Thrjoin => self.join(i, a),
                Call::Getaffinity => Poll::Ready(affinity::get(image, args)),
                Call::Gettid => Poll::Ready(Ok(self.0[i].tid as usize)),
                Call::Threrrno => Poll::Ready(Ok(self.0[i].errno)),
                Call::Futexwait | Call::Futexwake => self.futex(i, p.getpid(), image, call, a, b),
                Call::Fcntl if matches!(b, 5..=7 | 36..=38) => {
                    super::io::record::call(&mut self.0[i].lock_request, p, image, args, cx)
                }
                Call::Ttyio => p.tty_ioctl(image, a, b, c, cx),
                _ => file_call(image, p, cx, call, args, &mut self.0[i].tty_read),
            };
            if let Poll::Ready(value) = result {
                self.0[i].reply(value.map(|n| n as isize).unwrap_or_else(|e| e));
            }
        }
        if let Some(status) = self.deliver(image, p, cx) {
            return status;
        }
        self.0.retain(|t| !matches!(t.state, State::Joined));
        if self.0.iter().all(|t| matches!(t.state, State::Exited(_))) {
            return Poll::Ready(Ok(0));
        }
        for t in &mut self.0 {
            if matches!(t.state, State::Ready) {
                let cpu = t.registers.take().expect("runnable thread has registers");
                match user::submit(t.cpu, cpu, image.page_table(), &t.returned, cx.waker()) {
                    Ok(()) => t.state = State::Running,
                    Err(cpu) => {
                        // Dispatch storage cannot grow now. Retain the stopped
                        // thread so it can run after another CPU drains its queue.
                        t.registers = Some(cpu);
                        cx.waker().wake_by_ref();
                    }
                }
            }
        }
        Poll::Pending
    }
}
