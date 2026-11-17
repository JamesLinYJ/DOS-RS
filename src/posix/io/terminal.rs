//! The keyboard and text screen form one terminal device per virtual console.
use super::{Description, ProcessContext};
use crate::posix::{Waiters, executable::errno, poll, process, threads::clock::Timer};
use crate::{arch::user::AddressSpace, bios::pc, kernel_cell::KernelCell};
use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use core::task::{Context, Poll, Waker};
use dosrs_native_posix::{Errno, Result};

const WIRE: usize = 60;
const MAX_LINE: usize = 4096;
const ISIG: u32 = 1;
const ICANON: u32 = 2;
const ECHO: u32 = 8;
const ECHOE: u32 = 16;
const ECHOK: u32 = 32;
const ECHONL: u32 = 64;
const NOFLSH: u32 = 128;
const TOSTOP: u32 = 256;
const ECHOCTL: u32 = 512;
const IEXTEN: u32 = 32768;
const VINTR: usize = 0;
const VQUIT: usize = 1;
const VERASE: usize = 2;
const VKILL: usize = 3;
const VEOF: usize = 4;
const VTIME: usize = 5;
const VMIN: usize = 6;
const VSTART: usize = 8;
const VSTOP: usize = 9;
const VSUSP: usize = 10;
const VEOL: usize = 11;
const VWERASE: usize = 14;
const VLNEXT: usize = 15;
const VEOL2: usize = 16;

#[derive(Clone, Copy)]
struct Settings {
    input: u32,
    output: u32,
    control: u32,
    local: u32,
    cc: [u8; 32],
}
impl Settings {
    const DEFAULT: Self = {
        let mut cc = [0; 32];
        cc[VINTR] = 3;
        cc[VQUIT] = 28;
        cc[VERASE] = 8;
        cc[VKILL] = 21;
        cc[VEOF] = 4;
        cc[VMIN] = 1;
        cc[VSTART] = 17;
        cc[VSTOP] = 19;
        cc[VSUSP] = 26;
        cc[VWERASE] = 23;
        cc[VLNEXT] = 22;
        Self {
            input: 0o2400,
            output: 5,
            control: 0o277,
            local: ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | IEXTEN,
            cc,
        }
    };
    fn encode(self) -> [u8; WIRE] {
        let mut out = [0; WIRE];
        for (buf, v) in
            out[..16]
                .chunks_exact_mut(4)
                .zip([self.input, self.output, self.control, self.local])
        {
            buf.copy_from_slice(&v.to_le_bytes());
        }
        out[17..49].copy_from_slice(&self.cc);
        for off in [52, 56] {
            out[off..off + 4].copy_from_slice(&(self.control & 0o10017).to_le_bytes());
        }
        out
    }
    fn decode(buf: [u8; WIRE]) -> core::result::Result<Self, isize> {
        let word = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        // This virtual text console has no hardware baud clock, parity or output delay.
        // Reject unsupported processing instead of reporting flags with no effect.
        if buf[16] != 0
            || word(0) & !(0o40 | 0o100 | 0o200 | 0o400 | 0o2000 | 0o4000 | 3) != 0
            || word(4) & !(1 | 4 | 8) != 0
            || word(12)
                & !(ISIG
                    | ICANON
                    | ECHO
                    | ECHOE
                    | ECHOK
                    | ECHONL
                    | NOFLSH
                    | TOSTOP
                    | ECHOCTL
                    | IEXTEN)
                != 0
            || word(8) & (0o10017 | 0o260) != 0o277
            || word(8) & !(0o10017 | 0o60 | 0o200 | 0o2000 | 0o4000) != 0
        {
            return Err(-95);
        }
        Ok(Self {
            input: word(0),
            output: word(4),
            control: word(8),
            local: word(12),
            cc: buf[17..49].try_into().unwrap(),
        })
    }
    fn special(&self, ch: u8, slot: usize) -> bool {
        ch != 0 && self.cc[slot] == ch
    }
}

// DOS may regain control between native jobs. Terminal modes belong to the VT.
static SETTINGS: KernelCell<[Settings; pc::TERMINALS]> =
    KernelCell::new([Settings::DEFAULT; pc::TERMINALS]);

pub(in crate::posix) struct ReadClock {
    tid: u32,
    last: Option<(u8, u64)>,
    timer: Option<Timer>,
}
impl ReadClock {
    pub(in crate::posix) fn new(tid: u32) -> Self {
        Self {
            tid,
            last: None,
            timer: None,
        }
    }
    pub(in crate::posix) fn reset(&mut self, tid: u32) {
        *self = Self::new(tid);
    }
    fn expired(&mut self, interval: u8, arrival: u64, cx: &Context) -> Result<bool> {
        if self.last != Some((interval, arrival)) {
            self.timer = None;
            self.timer = Some(
                Timer::after(self.tid, u64::from(interval) * 100_000_000, cx)
                    .map_err(|_| Errno::Io)?,
            );
            self.last = Some((interval, arrival));
        }
        Ok(self.timer.as_ref().is_some_and(Timer::expired))
    }
}
struct Byte {
    value: u8,
    end: bool,
}
pub(in crate::posix) struct Terminal {
    id: usize,
    settings: Settings,
    editing: Vec<u8>,
    ready: VecDeque<Option<Byte>>,
    arrival: u64,
    literal: bool,
    stopped: bool,
    pending_echo: Vec<u8>,
    waiting_output: Waiters,
    pub(super) waiting: Waiters,
    pollers: poll::Subscribers,
    pub(super) session: u32,
    pub(super) foreground: u32,
}
impl Terminal {
    pub(in crate::posix) fn new() -> Self {
        let (id, _, _) = pc::terminal_geometry();
        Self {
            id,
            settings: SETTINGS.borrow()[id],
            editing: Vec::new(),
            ready: VecDeque::new(),
            arrival: 0,
            literal: false,
            stopped: false,
            pending_echo: Vec::new(),
            waiting: Waiters::default(),
            waiting_output: Waiters::default(),
            pollers: poll::Subscribers::default(),
            session: 0,
            foreground: 0,
        }
    }
    pub(super) fn attach(&mut self, pid: u32) {
        if self.session == 0 {
            self.session = pid;
            self.foreground = pid;
        }
        process::attach(pid, self.session, self.foreground);
    }
    fn output(&mut self, bytes: &[u8]) {
        if self.stopped {
            let len = bytes
                .len()
                .min(MAX_LINE.saturating_sub(self.pending_echo.len()));
            self.pending_echo.extend_from_slice(&bytes[..len]);
            return;
        }
        for &b in bytes {
            if self.settings.output & 1 != 0 {
                if b == b'\n' && self.settings.output & 4 != 0 {
                    crate::putc(b'\r');
                }
                if b == b'\r' && self.settings.output & 8 != 0 {
                    crate::putc(b'\n');
                    continue;
                }
            }
            crate::putc(b);
        }
    }
    fn echo(&mut self, ch: u8) {
        if self.settings.local & ECHO == 0 {
            return;
        }
        if self.settings.local & ECHOCTL != 0
            && (ch < 32 || ch == 127)
            && !matches!(ch, b'\n' | b'\t')
        {
            self.output(&[b'^', ch ^ 64]);
        } else {
            self.output(&[ch]);
        }
    }
    fn erase(&mut self) -> bool {
        let Some(ch) = self.editing.pop() else {
            return false;
        };
        if self.settings.local & (ECHO | ECHOE) == ECHO | ECHOE {
            self.output(b"\x08 \x08");
            if self.settings.local & ECHOCTL != 0 && ch < 32 && ch != b'\t' {
                self.output(b"\x08 \x08");
            }
        }
        true
    }
    fn finish_line(&mut self, end: Option<u8>) {
        if let Some(ch) = end {
            self.editing.push(ch);
        }
        let count = self.editing.len();
        if count == 0 {
            self.ready.push_back(None);
        } else {
            self.ready
                .extend(self.editing.drain(..).enumerate().map(|(i, value)| {
                    Some(Byte {
                        value,
                        end: i + 1 == count,
                    })
                }));
        }
    }
    fn clear_input(&mut self) {
        self.ready.clear();
        self.editing.clear();
        self.literal = false;
    }
    fn flow(&mut self, stopped: bool) {
        self.stopped = stopped;
        if !stopped {
            let bytes = core::mem::take(&mut self.pending_echo);
            self.output(&bytes);
            self.waiting_output.wake();
            self.pollers.wake();
        }
    }
    fn receive(&mut self, mut ch: u8) {
        let s = self.settings;
        if s.input & 0o40 != 0 {
            ch &= 127;
        }
        if ch == b'\r' {
            if s.input & 0o200 != 0 {
                return;
            }
            if s.input & 0o400 != 0 {
                ch = b'\n';
            }
        } else if ch == b'\n' && s.input & 0o100 != 0 {
            ch = b'\r';
        }
        let literal = core::mem::take(&mut self.literal);
        if !literal && s.input & 0o2000 != 0 {
            if s.special(ch, VSTOP) {
                self.flow(true);
                return;
            }
            if s.special(ch, VSTART) {
                self.flow(false);
                return;
            }
            if self.stopped && s.input & 0o4000 != 0 {
                self.flow(false);
            }
        }
        if !literal && s.local & ISIG != 0 {
            if let Some((_, sig)) = [(VINTR, 2), (VSUSP, 20), (VQUIT, 3)]
                .into_iter()
                .find(|(slot, _)| s.special(ch, *slot))
            {
                if s.local & NOFLSH == 0 {
                    self.clear_input();
                    self.pending_echo.clear();
                }
                self.echo(ch);
                if s.local & ECHO != 0 {
                    self.output(b"\n");
                }
                process::signals::terminal_input(self.foreground, sig);
                return;
            }
        }
        if self.ready.len() >= 16 * MAX_LINE {
            return;
        }
        if s.local & ICANON == 0 {
            self.echo(ch);
            self.ready.push_back(Some(Byte {
                value: ch,
                end: false,
            }));
            self.arrival = self.arrival.wrapping_add(1);
            return;
        }
        if !literal {
            if s.local & IEXTEN != 0 && s.special(ch, VLNEXT) {
                self.literal = true;
                return;
            }
            if s.special(ch, VERASE) {
                if self.erase() && s.local & ECHOE == 0 {
                    self.echo(ch);
                }
                return;
            }
            if s.special(ch, VKILL) {
                self.editing.clear();
                self.echo(ch);
                if s.local & (ECHO | ECHOK) == ECHO | ECHOK {
                    self.output(b"\n");
                }
                return;
            }
            if s.local & IEXTEN != 0 && s.special(ch, VWERASE) {
                while self.editing.last().is_some_and(u8::is_ascii_whitespace) {
                    self.erase();
                }
                while self
                    .editing
                    .last()
                    .is_some_and(|b| !b.is_ascii_whitespace())
                {
                    self.erase();
                }
                return;
            }
            if s.special(ch, VEOF) {
                self.finish_line(None);
                return;
            }
            if ch == b'\n' || s.special(ch, VEOL) || (s.local & IEXTEN != 0 && s.special(ch, VEOL2))
            {
                if ch == b'\n' && s.local & (ECHO | ECHONL) != 0 {
                    self.output(b"\n");
                } else {
                    self.echo(ch);
                }
                self.finish_line(Some(ch));
                return;
            }
        }
        if self.editing.len() < MAX_LINE - 1 {
            self.editing.push(ch);
            self.echo(ch);
        }
    }
    pub(in crate::posix) fn notify_input(&mut self) {
        while let Some(key) = pc::key(true) {
            let [scan, ch] = key.to_be_bytes();
            if ch != 0 {
                self.receive(ch);
            } else if let Some(seq) = match scan {
                0x48 => Some(b"\x1b[A"),
                0x50 => Some(b"\x1b[B"),
                0x4d => Some(b"\x1b[C"),
                0x4b => Some(b"\x1b[D"),
                _ => None,
            } {
                for &b in seq {
                    self.receive(b);
                }
            }
        }
        self.waiting.wake();
        if !self.ready.is_empty() {
            self.pollers.wake();
        }
    }
    pub(super) fn read(
        &mut self,
        cx: &Context,
        out: &mut [u8],
        timer: &mut ReadClock,
        nonblock: bool,
    ) -> Poll<Result<usize>> {
        if out.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let canonical = self.settings.local & ICANON != 0;
        let available = self.ready.len();
        if canonical {
            if available == 0 {
                return self.waiting.wait(cx);
            }
        } else if nonblock {
            if available == 0 {
                return Poll::Ready(Err(Errno::Again));
            }
        } else {
            let min = usize::from(self.settings.cc[VMIN]).min(out.len());
            let time = self.settings.cc[VTIME];
            let pending = if min == 0 {
                available == 0 && time != 0 && !timer.expired(time, 0, cx)?
            } else if available >= min {
                false
            } else if time == 0 || available == 0 {
                timer.reset(timer.tid);
                true
            } else {
                !timer.expired(time, self.arrival, cx)?
            };
            if pending {
                return self.waiting.wait(cx);
            }
        }
        let mut count = 0;
        while count < out.len() {
            let Some(byte) = self.ready.pop_front() else {
                break;
            };
            let Some(byte) = byte else { break };
            out[count] = byte.value;
            count += 1;
            if canonical && byte.end {
                break;
            }
        }
        Poll::Ready(Ok(count))
    }
    pub(super) fn write(&mut self, cx: &Context, bytes: &[u8]) -> Poll<Result<usize>> {
        if self.stopped && !bytes.is_empty() {
            return self.waiting_output.wait(cx);
        }
        self.output(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    pub(super) fn readiness(
        &mut self,
        events: i16,
        output: bool,
        waker: Option<&Arc<Waker>>,
    ) -> core::result::Result<i16, isize> {
        let (wanted, ready) = if output {
            (poll::WRITE, !self.stopped)
        } else {
            let min = if self.settings.local & ICANON == 0 && self.settings.cc[VTIME] == 0 {
                usize::from(self.settings.cc[VMIN]).max(1)
            } else {
                1
            };
            (poll::READ, self.ready.len() >= min)
        };
        if ready {
            Ok(events & wanted)
        } else {
            if events & wanted != 0 {
                self.pollers.subscribe(waker)?;
            }
            Ok(0)
        }
    }
    fn set(&mut self, next: Settings, flush: bool) {
        if flush {
            self.clear_input();
        } else if self.settings.local & ICANON != 0 && next.local & ICANON == 0 {
            self.ready.retain(Option::is_some);
            self.ready.extend(
                self.editing
                    .drain(..)
                    .map(|value| Some(Byte { value, end: false })),
            );
        } else if self.settings.local & ICANON == 0
            && next.local & ICANON != 0
            && let Some(Some(last)) = self.ready.back_mut()
        {
            last.end = true;
        }
        if next.input & 0o2000 == 0 {
            self.flow(false);
        }
        self.settings = next;
        SETTINGS.borrow()[self.id] = next;
        self.waiting.wake();
        self.pollers.wake();
    }
    pub(super) fn stop_background_output(&self) -> bool {
        self.settings.local & TOSTOP != 0
    }
}

impl ProcessContext {
    pub(super) fn background_output(&self, tty: &Terminal) -> core::result::Result<bool, isize> {
        let pgid = process::group(self.pid, 0, false)? as u32;
        let sid = process::group(self.pid, 0, true)? as u32;
        if sid != tty.session
            || pgid == tty.foreground
            || self.caller_mask & process::signals::bit(22) != 0
            || process::signals::disposition(self.pid, 22, false).handler == 1
        {
            return Ok(false);
        }
        process::signals::terminal_input(pgid, 22);
        Ok(true)
    }
    pub(in crate::posix) fn tty_ioctl(
        &self,
        image: &mut AddressSpace,
        fd: usize,
        request: usize,
        arg: usize,
        cx: &Context,
    ) -> Poll<core::result::Result<usize, isize>> {
        let handle = self.get(fd).map_err(errno)?;
        let desc = handle.borrow();
        let tty = match &*desc {
            Description::Input(tty) | Description::Output(tty) => tty,
            _ => return Poll::Ready(Err(-25)),
        };
        let mut t = tty.borrow();
        if matches!(request,0x5402..=0x5404 | 0x5409..=0x540b | 0x5414)
            && self.background_output(&t)?
        {
            return t.waiting.wait(cx);
        }
        if (matches!(request, 0x5403 | 0x5404) || request == 0x5409 && arg != 0)
            && !t.pending_echo.is_empty()
        {
            return t.waiting_output.wait(cx);
        }
        // Writes to the screen complete synchronously. Only keyboard echo can queue
        // while output is stopped; application writes wait for flow to resume.
        let res = match request {
            0x5401 => image.write(arg, &t.settings.encode()).map_err(|_| -14),
            0x5402..=0x5404 => {
                let mut b = [0; WIRE];
                image.read(arg, &mut b).map_err(|_| -14isize)?;
                let next = Settings::decode(b)?;
                t.set(next, request == 0x5404);
                Ok(())
            }
            0x5413 | 0x5414 => {
                let (_, rows, cols) = pc::terminal_geometry();
                let mut b = [0u8; 8];
                b[..2].copy_from_slice(&rows.to_le_bytes());
                b[2..4].copy_from_slice(&cols.to_le_bytes());
                if request == 0x5413 {
                    image.write(arg, &b).map_err(|_| -14)
                } else {
                    let mut requested = [0; 8];
                    image.read(arg, &mut requested).map_err(|_| -14isize)?;
                    if requested == b { Ok(()) } else { Err(-95) }
                }
            }
            0x5409 => {
                if arg == 0 {
                    Err(-95)
                } else {
                    Ok(())
                }
            } // tcdrain; no serial break on VGA
            0x540a => match arg {
                0 | 1 => {
                    t.flow(arg == 0);
                    Ok(())
                }
                2 | 3 => Err(-95),
                _ => Err(-22),
            },
            0x540b => match arg {
                0..=2 => {
                    if arg != 0 {
                        t.pending_echo.clear();
                        t.waiting_output.wake();
                    }
                    if arg != 1 {
                        t.clear_input();
                        t.waiting.wake();
                    }
                    Ok(())
                }
                _ => Err(-22),
            },
            _ => Err(-25),
        };
        Poll::Ready(res.map(|()| 0))
    }
}
