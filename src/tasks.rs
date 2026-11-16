//! Boot-CPU native task groups, wakeups and per-terminal foreground ownership.
#![forbid(unsafe_code)]
use crate::{bios::pc, kernel_cell::KernelCell, posix::Job, vfs::Directories};
use alloc::{boxed::Box, sync::Arc, task::Wake, vec::Vec};
use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};

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
pub(crate) struct Group {
    children: Vec<Child>,
}

impl Group {
    pub fn new(capacity: usize) -> Self {
        Self {
            children: Vec::with_capacity(capacity),
        }
    }
    pub fn spawn(&mut self, main: impl Future<Output = u8> + Send + 'static) {
        self.children.push(Child {
            ready: Arc::new(Ready(AtomicBool::new(true))),
            main: Some(Box::pin(main)),
            status: 0,
        });
    }
    /// Poll a ready child at most once per scheduler round. Dropping a completed
    /// continuation closes its descriptors before its peers run again.
    pub fn tick(&mut self) -> Option<u8> {
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

static JOBS: [KernelCell<Option<Job>>; pc::TERMINALS] =
    [const { KernelCell::new(None) }; pc::TERMINALS];

pub(crate) enum Command {
    Unknown,
    Started,
    Exited(u8),
    Failed(&'static str),
}

pub(crate) fn launch(id: usize, scheduled: bool, line: &[u8], cwd: Directories) -> Command {
    let Some(result) = Job::launch(line, cwd) else {
        return Command::Unknown;
    };
    let mut job = match result {
        Ok(job) => job,
        Err(error) => return Command::Failed(error),
    };
    if scheduled {
        let mut slot = JOBS[id].borrow();
        if slot.is_some() {
            return Command::Failed("terminal already has a foreground job");
        }
        *slot = Some(job);
        return Command::Started;
    }
    loop {
        if let Some(code) = job.tick() {
            return Command::Exited(code);
        }
    }
}

pub(crate) fn active(id: usize) -> bool {
    JOBS[id].borrow().is_some()
}
pub(crate) fn poll_terminal(id: usize) -> Option<Poll<u8>> {
    let mut slot = JOBS[id].borrow();
    let job = slot.as_mut()?;
    pc::route_output(id);
    match job.tick() {
        Some(code) => {
            *slot = None;
            Some(Poll::Ready(code))
        }
        None => Some(Poll::Pending),
    }
}

pub(crate) fn interactive() -> ! {
    crate::fs::boot();
    pc::enable_terminals();
    let mut dos = crate::guest::run::Terminals::new();
    loop {
        let foreground = pc::poll_keyboard(|id, code| dos.input(id, code));
        for id in 0..pc::TERMINALS {
            match poll_terminal(id) {
                Some(Poll::Ready(code)) => dos.child_exit(id, code),
                Some(Poll::Pending) => {}
                None => dos.poll(id, foreground),
            }
        }
        core::hint::spin_loop();
    }
}
