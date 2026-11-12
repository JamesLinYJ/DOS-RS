//! Kernel-driven shell startup, virtual terminals and round-robin guest execution.
use super::{
    CpuState, End, GuestRam, SHELL_SEGMENT, Session, drive, drivers, finish_child, load_root,
};
use crate::bios::pc;
use crate::guest::{command, disk, memory};
use crate::kernel_cell::KernelCell;

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
    cwd: u16,
    line: [u8; 126],
    used: usize,
}

impl Task {
    fn boot(id: usize, ram: &mut GuestRam) -> Self {
        let mut session = Session::new();
        session.terminal = id;
        session.scheduled = true;
        disk::set_working(0);
        crate::puts("DOS-RS terminal ");
        crate::bios::mscon::write_number((id + 1) as u8);
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
            cwd: 0,
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

    fn tick(&mut self, ram: &mut GuestRam) {
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
            match drive(cpu, ram, &mut self.session, 1024) {
                End::Loop => {}
                End::Switch => self.accept_child(),
                End::Waiting => self.waiting = true,
                result => {
                    let code = match result {
                        End::Stopped(code) => code,
                        End::Fault(op) => {
                            crate::puts("Program stopped at opcode ");
                            super::put_hex8(op);
                            crate::puts("\n");
                            0xff
                        }
                        _ => {
                            crate::puts("Unsupported interrupt.\n");
                            0xff
                        }
                    };
                    self.session.last_code = u16::from(code);
                    self.cpu = if self.session.child_depth == 0 {
                        None
                    } else {
                        finish_child(&mut self.session, ram)
                    };
                    self.prompt = false;
                }
            }
            return;
        }
        if !self.prompt {
            let mut path = [0; 64];
            let length = disk::current_path(&mut path);
            crate::puts("A:\\");
            crate::bios::mscon::con_write_bytes(&path[..length]);
            crate::puts("> ");
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

pub fn interactive() -> ! {
    disk::boot();
    pc::enable_terminals();
    loop {
        let foreground = pc::poll_keyboard();
        for (id, slot) in TASKS.iter().enumerate() {
            let mut task = slot.borrow();
            if task.is_none() && id != foreground {
                continue;
            }
            pc::route_output(id);
            let mut ram = memory::session_ram(id);
            let task = task.get_or_insert_with(|| Task::boot(id, &mut ram));
            disk::set_working(task.cwd);
            task.tick(&mut ram);
            task.cwd = disk::working_directory();
        }
        core::hint::spin_loop();
    }
}
