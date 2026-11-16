//! Native processes on the boot CPU. DOS tasks and native jobs share the
//! terminal scheduler and FAT volumes. Each native stage has its own Rust
//! continuation and descriptor table, including inherited open descriptions.
#![forbid(unsafe_code)]
mod executable;
mod io;
mod pipe;
mod shell;

use crate::kernel_cell::KernelCell;
use crate::{tasks::Group, vfs::Directories};
use alloc::{sync::Arc, vec::Vec};
use core::task::{Context, Poll, Waker};
use dosrs_native_posix::{Open, Program};
enum Native {
    Builtin(Program),
    Disk(executable::Executable),
}
use io::{ProcessContext, Terminal};

#[derive(Default)]
struct Waiters(Vec<Waker>);
impl Waiters {
    fn wait<T>(&mut self, cx: &Context) -> Poll<T> {
        if !self.0.iter().any(|w| w.will_wake(cx.waker())) {
            self.0.push(cx.waker().clone());
        }
        Poll::Pending
    }
    fn wake(&mut self) {
        for w in self.0.drain(..) {
            w.wake();
        }
    }
}

pub(crate) struct Job {
    terminal: Arc<KernelCell<Terminal>>,
    group: Group,
}

impl Job {
    pub fn launch(line: &[u8], cwd: Directories) -> Option<Result<Self, &'static str>> {
        let line = core::str::from_utf8(line).ok()?;
        let name = line.split([' ', '\t', '|', '>', '<']).next()?;
        if Program::lookup(name).is_none() && !name.to_ascii_lowercase().ends_with(".elf") {
            return None;
        }
        Some(shell::parse(line, cwd).and_then(|stages| Self::spawn(stages, cwd)))
    }

    fn spawn(stages: Vec<shell::Stage>, cwd: Directories) -> Result<Self, &'static str> {
        let terminal = Arc::new(KernelCell::new(Terminal::new()));
        let parent = ProcessContext::terminal(terminal.clone(), cwd);
        let mut group = Group::new(stages.len());
        let mut next_input = None;
        let count = stages.len();
        for (i, stage) in stages.into_iter().enumerate() {
            let mut descriptors = parent.inherit().map_err(|e| e.message())?;
            if let Some(input) = next_input.take() {
                descriptors.set(0, input);
            }
            if i + 1 < count {
                let (read, write) = io::pipe_handles();
                descriptors.set(1, write);
                next_input = Some(read);
            }
            if let Some(path) = stage.input {
                descriptors
                    .redirect(0, &path, Open::READ)
                    .map_err(|e| e.message())?;
            }
            if let Some((path, mode)) = stage.output {
                descriptors
                    .redirect(1, &path, mode)
                    .map_err(|e| e.message())?;
            }
            group.spawn(async move {
                let status = match stage.program {
                    Native::Builtin(p) => p.run(&mut descriptors, &stage.argv).await,
                    Native::Disk(task) => task.run(&mut descriptors).await,
                };
                if descriptors.finish().is_err() {
                    return 1;
                }
                status
            });
        }
        Ok(Self { terminal, group })
    }

    pub fn notify_input(&self) {
        self.terminal.borrow().notify_input();
    }

    pub fn tick(&mut self) -> Option<u8> {
        self.group.tick()
    }
}
