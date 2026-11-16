//! Native processes on the boot CPU. DOS tasks and native jobs share the
//! terminal scheduler and FAT volumes. Each native stage has its own Rust
//! continuation and descriptor table, including inherited open descriptions.
#![forbid(unsafe_code)]
mod io;
mod pipe;
mod shell;

use crate::kernel_cell::KernelCell;
use alloc::{boxed::Box, sync::Arc, task::Wake, vec::Vec};
use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};
use dosrs_native_posix::{Open, Program};
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
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

struct Ready(AtomicBool);
impl Wake for Ready {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }
}
struct Child {
    ready: Arc<Ready>,
    main: Option<Pin<Box<dyn Future<Output = u8> + Send>>>,
    status: u8,
}
pub(crate) struct Job {
    terminal: Arc<KernelCell<Terminal>>,
    children: Vec<Child>,
}

impl Job {
    pub fn launch(line: &[u8]) -> Option<Result<Self, &'static str>> {
        let line = core::str::from_utf8(line).ok()?;
        let name = line.split([' ', '\t', '|', '>', '<']).next()?;
        Program::lookup(name)?;
        Some(shell::parse(line).and_then(Self::spawn))
    }

    fn spawn(stages: Vec<shell::Stage>) -> Result<Self, &'static str> {
        let terminal = Arc::new(KernelCell::new(Terminal::new()));
        let parent = ProcessContext::terminal(terminal.clone());
        let mut children = Vec::with_capacity(stages.len());
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
            if let Some(path) = stage.output {
                descriptors
                    .redirect(1, &path, Open::OUTPUT)
                    .map_err(|e| e.message())?;
            }
            children.push(Child {
                ready: Arc::new(Ready(AtomicBool::new(true))),
                main: Some(Box::pin(async move {
                    let status = stage.program.run(&mut descriptors, &stage.argv).await;
                    if descriptors.finish().is_err() {
                        return 1;
                    }
                    status
                })),
                status: 0,
            });
        }
        Ok(Self { terminal, children })
    }

    /// Poll a ready child at most once per scheduler round. Dropping a completed
    /// continuation closes its descriptors before its peers run again.
    pub fn tick(&mut self) -> Option<u8> {
        self.terminal.borrow().keyboard();
        let mut running = false;
        for child in &mut self.children {
            let Some(main) = child.main.as_mut() else {
                continue;
            };
            if child.ready.0.swap(false, Ordering::AcqRel) {
                let waker = Waker::from(child.ready.clone());
                if let Poll::Ready(code) = main.as_mut().poll(&mut Context::from_waker(&waker)) {
                    child.status = code;
                    child.main = None;
                    continue;
                }
            }
            running = true;
        }
        if running {
            None
        } else {
            self.children.last().map(|c| c.status)
        }
    }
}
