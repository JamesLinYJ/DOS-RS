//! Process-owned threads. CPUs execute runnable threads in parallel. At a
//! quantum boundary, all CPUs release the process pages before the kernel
//! reads a syscall buffer, changes a mapping or tears down the process.
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
    Call(Call, [usize; 3]),
    Futex(usize),
    Exited(u8),
    Joined,
}

struct Member {
    tid: u32,
    cpu: usize,
    stack: Range<usize>,
    errno: usize,
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
            cpu: user::assign_cpu(),
            stack,
            errno,
            registers: Some(registers),
            returned: Completion::new()?,
            state: State::Ready,
        })
    }
    fn reply(&mut self, value: isize) {
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
        let mut threads = Vec::new();
        threads
            .try_reserve(1)
            .map_err(|_| "insufficient thread memory")?;
        threads.push(Member::new(0, STACK..END, 0, main)?);
        Ok(Self(threads))
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

    fn collect(&mut self, image: &AddressSpace, pid: u32) -> Result<(), (u8, String)> {
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
        }
        self.collect(image, p.getpid())?;
        for i in 0..self.0.len() {
            let State::Call(call, args) = self.0[i].state else {
                continue;
            };
            let [a, b, c] = args;
            let result = match call {
                Call::Exit => return Poll::Ready(Ok(a as u8)),
                Call::Threxit => {
                    self.0[i].state = State::Exited(a as u8);
                    self.0[i].registers = None;
                    cx.waker().wake_by_ref();
                    continue;
                }
                Call::Thrspawn => Poll::Ready(self.spawn(image, a, b, c)),
                Call::Thrjoin => self.join(i, a),
                Call::Gettid => Poll::Ready(Ok(self.0[i].tid as usize)),
                Call::Threrrno => Poll::Ready(Ok(self.0[i].errno)),
                Call::Futexwait | Call::Futexwake => self.futex(i, p.getpid(), image, call, a, b),
                _ => file_call(image, p, cx, call, args),
            };
            if let Poll::Ready(value) = result {
                self.0[i].reply(value.map(|n| n as isize).unwrap_or_else(|e| e));
            }
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
