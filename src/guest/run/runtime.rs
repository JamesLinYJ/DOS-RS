//! Kernel-driven shell startup, virtual terminals and round-robin guest execution.
use super::{
    CpuState, End, GuestRam, SHELL_SEGMENT, Session, drive, drivers, finish_child, load_root,
};
use super::{Outcome, handle_event, step};
use crate::bios::pc;
use crate::guest::{command, disk, memory};
use crate::kernel_cell::KernelCell;
use core::sync::atomic::{
    AtomicU8,
    Ordering::{Acquire, Release},
};

fn start(ram: &mut GuestRam, session: &mut Session) -> Option<CpuState> {
    ram.clear();
    let mut image = [0u8; 512];
    let length = disk::read_file(b"COMMAND COM", &mut image)?;
    let cpu = load_root(
        &image[..length],
        b"COMMAND COM",
        SHELL_SEGMENT,
        &[],
        ram,
        session,
    )?;
    drivers::configure(ram, session);
    session.arena.publish(ram);
    Some(cpu)
}

/// Bounded batch entry for unattended workloads and the self-test harness.
pub fn run_shell() -> bool {
    disk::boot();
    let ram = &mut *memory::storage();
    let mut session = Session::new();
    let Some(mut cpu) = start(ram, &mut session) else {
        crate::puts("Cannot load COMMAND.COM.\n");
        return false;
    };
    if cfg!(feature = "batch-console") {
        crate::puts("Running AUTOEXEC.BAT\n\n");
    }
    matches!(drive(&mut cpu, ram, &mut session, 100_000), End::Stopped(0))
}

struct Task {
    session: Session,
    cpu: Option<CpuState>,
    waiting: bool,
    prompt: bool,
    cwd: disk::Directories,
    line: [u8; 126],
    used: usize,
}

impl Task {
    fn boot(id: usize, ram: &mut GuestRam) -> Self {
        let mut session = Session::new();
        session.terminal = id;
        session.scheduled = true;
        disk::restore(disk::Directories::ROOTS);
        let attr = pc::color(0x0b);
        crate::puts("DOS-RS terminal ");
        crate::bios::mscon::write_number((id + 1) as u8);
        pc::color(attr);
        crate::puts("   Alt+F1..F4 to switch\n");
        let cpu = start(ram, &mut session);
        if cpu.is_none() {
            crate::puts("Cannot load COMMAND.COM.\n");
        }
        Self {
            session,
            cpu,
            waiting: false,
            prompt: false,
            cwd: disk::directories(),
            line: [0; 126],
            used: 0,
        }
    }

    fn accept_child(&mut self) {
        if let Some(child) = self.session.pending.take() {
            let parent = self.session.parents[self.session.child_depth as usize - 1]
                .as_mut()
                .unwrap();
            parent.registers = self.cpu.replace(child);
        }
    }

    fn tick(&mut self, ram: &mut GuestRam, completed: Option<(Outcome, u16)>) {
        if self.waiting && pc::key(false).is_none() {
            return;
        }
        self.waiting = false;
        let mut line = [0; 512];
        if let Some(n) = self
            .session
            .batches
            .line(self.session.child_depth, &mut line)
        {
            command::execute(&line[..n], ram, &mut self.session, 0, true);
            self.accept_child();
            return;
        }
        if let Some(cpu) = self.cpu.as_mut() {
            let end = if let Some((event, ip)) = completed {
                handle_event(event, ip, cpu, ram, &mut self.session).unwrap_or_else(|| {
                    if self.session.pending.is_some() {
                        End::Switch
                    } else {
                        End::Loop
                    }
                })
            } else if crate::platform::worker(self.session.terminal) == 0 {
                drive(cpu, ram, &mut self.session, 1024)
            } else {
                End::Loop
            };
            match end {
                End::Loop => {}
                End::Switch => self.accept_child(),
                End::Waiting => self.waiting = true,
                result => {
                    let code = match result {
                        End::Stopped(code) => code,
                        End::Fault(op) => {
                            crate::puts("Program stopped at opcode ");
                            super::put_hex8(op);
                            crate::puts(" at ");
                            crate::put_hex16(cpu.selector(super::Segment::Code));
                            crate::putc(b':');
                            crate::put_hex16(cpu.instruction());
                            crate::puts("  ");
                            for n in 0..6 {
                                super::put_hex8(
                                    cpu.load_byte(
                                        ram,
                                        super::Segment::Code,
                                        cpu.instruction().wrapping_add(n),
                                    )
                                    .unwrap_or(0),
                                );
                            }
                            crate::puts("\n");
                            0xff
                        }
                        End::Other(vector) => {
                            crate::puts("Unsupported interrupt ");
                            super::put_hex8(vector);
                            crate::puts(" AX=");
                            crate::put_hex16(cpu.wide(super::Slot::Accumulator));
                            crate::puts("\n");
                            0xff
                        }
                        _ => unreachable!(),
                    };
                    self.session.last_code = u16::from(code);
                    self.cpu = if self.session.child_depth == 0 {
                        None
                    } else {
                        finish_child(
                            &mut self.session,
                            ram,
                            None,
                            matches!(result, End::Stopped(_)),
                        )
                    };
                    self.prompt = false;
                }
            }
            return;
        }
        if !self.prompt {
            #[cfg(feature = "pc386")]
            if ram.graphics.as_ref().is_some_and(|g| g.active) {
                let mut regs = CpuState::at_com(SHELL_SEGMENT);
                regs.set_wide(super::Slot::Accumulator, 3);
                super::serve_bios(&mut regs, ram, 0x10);
            }
            let mut path = [0; 64];
            let length = disk::current_path(&mut path);
            let attr = pc::prompt_color();
            crate::putc(b'A' + self.session.drive);
            crate::puts(":\\");
            crate::bios::mscon::con_write_bytes(&path[..length]);
            crate::puts("> ");
            pc::color(attr);
            self.prompt = true;
        }
        let Some(key) = pc::key(true) else { return };
        match key as u8 {
            b'\r' => {
                crate::puts("\n");
                command::execute(&self.line[..self.used], ram, &mut self.session, 0, false);
                self.used = 0;
                self.prompt = false;
                self.accept_child();
            }
            8 if self.used > 0 => {
                self.used -= 1;
                crate::bios::mscon::con_write_bytes(b"\x08 \x08");
            }
            c @ 32..=126 if self.used < self.line.len() => {
                self.line[self.used] = c;
                self.used += 1;
                crate::bios::mscon::con_write_bytes(&[c]);
            }
            _ => {}
        }
    }
}

static TASKS: [KernelCell<Option<Task>>; pc::TERMINALS] =
    [const { KernelCell::new(None) }; pc::TERMINALS];

// Only the assigned worker touches RUNNING guest RAM. The boot processor
// services I/O, DOS and shell events after acquiring the completed context.
const IDLE: u8 = 0;
const RUNNING: u8 = 1;
const DONE: u8 = 2;
struct Slice {
    cpu: CpuState,
    event: Outcome,
    instruction: u16,
}
static STATE: [AtomicU8; pc::TERMINALS] = [const { AtomicU8::new(IDLE) }; pc::TERMINALS];
static SLICES: [KernelCell<Option<Slice>>; pc::TERMINALS] =
    [const { KernelCell::new(None) }; pc::TERMINALS];

pub(crate) fn worker_tick(worker: usize) -> bool {
    let mut worked = false;
    for (id, state) in STATE.iter().enumerate() {
        if state.load(Acquire) != RUNNING || crate::platform::worker(id) != worker {
            continue;
        }
        {
            let mut handoff = SLICES[id].borrow();
            let slice = handoff.as_mut().unwrap();
            let mut ram = memory::session_ram(id);
            (slice.event, slice.instruction) = step::slice(&mut slice.cpu, &mut ram, 1024);
        }
        state.store(DONE, Release);
        worked = true;
    }
    worked
}

pub fn interactive() -> ! {
    disk::boot();
    pc::enable_terminals();
    #[cfg(feature = "pc386")]
    let mut painted: Option<usize> = None;
    #[cfg(feature = "pc386")]
    let mut graphical = [false; pc::TERMINALS];
    #[cfg(feature = "pc386")]
    let mut scans: [alloc::collections::VecDeque<u8>; pc::TERMINALS] =
        core::array::from_fn(|_| alloc::collections::VecDeque::new());
    loop {
        let foreground = pc::poll_keyboard(|_id, _code| {
            #[cfg(feature = "pc386")]
            if graphical[_id] {
                if scans[_id].len() < 128 {
                    scans[_id].push_back(_code);
                }
                return true;
            }
            false
        });
        for (id, slot) in TASKS.iter().enumerate() {
            let state = STATE[id].load(Acquire);
            if state == RUNNING {
                continue;
            }
            let mut task = slot.borrow();
            if task.is_none() && id != foreground {
                continue;
            }
            pc::route_output(id);
            let mut ram = memory::session_ram(id);
            let task = task.get_or_insert_with(|| Task::boot(id, &mut ram));
            disk::restore(task.cwd);
            let completed = if state == DONE {
                let slice = SLICES[id].borrow().take().unwrap();
                task.cpu = Some(slice.cpu);
                STATE[id].store(IDLE, Release);
                Some((slice.event, slice.instruction))
            } else {
                None
            };
            #[cfg(feature = "pc386")]
            {
                // Only touch the controller after the worker has returned the session.
                graphical[id] = ram.graphics.as_ref().is_some_and(|g| g.active);
                if graphical[id]
                    && let Some(at) = &mut ram.at
                {
                    for code in scans[id].drain(..) {
                        at.key(code);
                    }
                } else {
                    scans[id].clear();
                }
            }
            #[cfg(feature = "pc386")]
            if id == foreground {
                if let Some(g) = &mut ram.graphics
                    && g.active
                {
                    g.render(painted != Some(id));
                    painted = Some(id);
                } else if painted.take().is_some() {
                    pc::restore_text();
                }
            }
            ram.updated_text(pc::guest_text_row);
            task.tick(&mut ram, completed);
            task.cwd = disk::directories();
            if crate::platform::worker(id) != 0
                && !task.waiting
                && !task.session.batches.ready(task.session.child_depth)
                && let Some(cpu) = task.cpu.take()
            {
                *SLICES[id].borrow() = Some(Slice {
                    cpu,
                    event: Outcome::Continue,
                    instruction: 0,
                });
                drop(ram);
                STATE[id].store(RUNNING, Release);
            }
        }
        core::hint::spin_loop();
    }
}
