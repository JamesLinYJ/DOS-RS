//! Native processes on the boot CPU. DOS tasks and native jobs share the
//! terminal scheduler and FAT volumes. Each native stage has its own Rust
//! continuation and descriptor table, including inherited open descriptions.
#![forbid(unsafe_code)]
mod executable;
mod io;
mod groups;
mod paths;
mod pipe;
mod poll;
mod process;
mod shell;
mod threads;
mod uts;
pub(crate) use threads::clock::tick as poll_timers;

use crate::kernel_cell::KernelCell;
use crate::{tasks::Group, vfs::Directories};
use alloc::{sync::Arc, vec::Vec};
use core::task::{Context, Poll, Waker};
use dosrs_native_posix::{Errno, Open};
enum Native {
    FdCheck,
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
    new_processes: Arc<process::NewProcesses>,
}

fn find_utility(
    cwd: Directories,
    name: &str,
) -> Result<(crate::vfs::File, alloc::string::String), Errno> {
    use crate::vfs::posix;
    for dir in ["/bin", "/dos/a/bin"] {
        let path = alloc::format!("{dir}/{name}");
        match posix::open(
            cwd,
            posix::initial(cwd),
            path.as_bytes(),
            Open::READ,
            0,
            posix::ADMIN,
            true,
        ) {
            Ok((file, _)) => return Ok((file, path)),
            Err(Errno::NoEntry | Errno::NotDirectory) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(Errno::NoEntry)
}

impl Job {
    pub fn launch(line: &[u8], cwd: Directories) -> Option<Result<Self, &'static str>> {
        let line = core::str::from_utf8(line).ok()?;
        let name = line.split([' ', '\t', '|', '>', '<']).next()?;
        let shell = matches!(name, "sh" | "/bin/sh");
        let external = name.to_ascii_lowercase().ends_with(".elf");
        let diagnostic = name == "fdcheck";
        if !(shell || external || diagnostic) {
            if name.is_empty() || name.contains(['/', ':', '.', '\\']) {
                return None;
            }
            match find_utility(cwd, name) {
                Ok(_) => {}
                Err(Errno::NoEntry | Errno::NotDirectory | Errno::NameTooLong) => return None,
                Err(error) => return Some(Err(error.message())),
            }
        }
        if !external && !diagnostic {
            return Some((|| {
                let argv = if shell {
                    shell::shell_args(line)?
                } else {
                    alloc::vec!["sh".into(), "-c".into(), line.into()]
                };
                let (file, _path) = find_utility(cwd, "sh").map_err(|e| e.message())?;
                let env = [alloc::string::String::from("PATH=/bin:/dos/a/bin")];
                let task = executable::Executable::from_file(
                    file, &argv, &env, 0, 0, dosrs_native_exec::STACK_MAX,
                )?;
                #[cfg(feature = "exec-trace")]
                for byte in alloc::format!("EXEC foreground path={_path}\r\n").bytes() {
                    crate::bios::pc::serial_write(0x3f8, byte);
                }
                Self::spawn(
                    alloc::vec![shell::Stage {
                        program: Native::Disk(task),
                        argv,
                        input: None,
                        output: None,
                    }],
                    cwd,
                )
            })());
        }
        Some(shell::parse(line, cwd).and_then(|stages| Self::spawn(stages, cwd)))
    }

    fn spawn(stages: Vec<shell::Stage>, cwd: Directories) -> Result<Self, &'static str> {
        let terminal = Arc::new(KernelCell::new(Terminal::new()));
        let new_processes = Arc::new(KernelCell::new(Vec::new()));
        let parent =
            ProcessContext::terminal(terminal.clone(), cwd, Arc::downgrade(&new_processes));
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
            group.spawn(i + 1 == count, async move {
                let status = match stage.program {
                    Native::FdCheck => {
                        dosrs_native_posix::fdcheck(&mut descriptors, &stage.argv).await
                    }
                    Native::Disk(task) => task.run(&mut descriptors).await,
                };
                descriptors.complete(status)
            });
        }
        Ok(Self {
            terminal,
            group,
            new_processes,
        })
    }

    pub fn notify_input(&self) {
        self.terminal.borrow().notify_input();
    }

    pub fn tick(&mut self) -> Option<u8> {
        for (task, mut child) in self.new_processes.borrow().drain(..) {
            self.group.spawn(false, async move {
                let status = task.run(&mut child).await;
                child.complete(status)
            });
        }
        let status = self.group.tick();
        if self.new_processes.borrow().is_empty() {
            status
        } else {
            None
        }
    }
}
