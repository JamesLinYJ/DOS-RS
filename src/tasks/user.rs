//! Per-CPU run queues for user-mode execution. The foreground executor handles
//! system calls after all threads of the calling process have left user mode.
use crate::arch::user::{Stop, Thread, UserPageTable};
use crate::{kernel_cell::KernelCell, platform};
use alloc::{collections::VecDeque, sync::Arc};
use core::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    task::Waker,
};

pub(crate) struct Returned {
    pub thread: Thread,
    pub outcome: Result<Stop, &'static str>,
    #[cfg(feature = "thread-trace")]
    pub interval: (usize, u64, u64),
}
pub(crate) struct Completion(KernelCell<Option<Returned>>);

impl Completion {
    pub(crate) fn new() -> Result<Arc<Self>, &'static str> {
        Arc::try_new(Self(KernelCell::new(None))).map_err(|_| "insufficient thread memory")
    }
    pub(crate) fn ready(&self) -> bool {
        self.0.borrow().is_some()
    }
    pub(crate) fn take(&self) -> Option<Returned> {
        self.0.borrow().take()
    }
}

struct Work {
    thread: Thread,
    table: Arc<UserPageTable>,
    result: Arc<Completion>,
    waker: Waker,
}

static NEXT_CPU: AtomicUsize = AtomicUsize::new(0);
pub(crate) fn assign_cpu() -> usize {
    NEXT_CPU.fetch_add(1, Ordering::Relaxed) % platform::cpu_count()
}
static EXECUTING: [AtomicBool; platform::MAX_CPUS] =
    [const { AtomicBool::new(false) }; platform::MAX_CPUS];
pub(crate) fn executing(slot: usize) -> bool {
    EXECUTING[slot].load(Ordering::Acquire)
}
struct Queue {
    first: Option<Work>,
    rest: VecDeque<Work>,
}
static QUEUES: [KernelCell<Queue>; platform::MAX_CPUS] = [const {
    KernelCell::new(Queue {
        first: None,
        rest: VecDeque::new(),
    })
}; platform::MAX_CPUS];

pub(crate) fn submit(
    slot: usize,
    thread: Thread,
    table: Arc<UserPageTable>,
    result: &Arc<Completion>,
    waker: &Waker,
) -> Result<(), Thread> {
    let mut queue = QUEUES[slot].borrow();
    if queue.first.is_some() && queue.rest.try_reserve(1).is_err() {
        return Err(thread);
    }
    let work = Work {
        thread,
        table,
        result: result.clone(),
        waker: waker.clone(),
    };
    if queue.first.is_none() {
        queue.first = Some(work);
    } else {
        queue.rest.push_back(work);
    }
    drop(queue);
    platform::wake_cpu(slot);
    Ok(())
}

pub(crate) fn run_one(slot: usize) {
    let mut queue = QUEUES[slot].borrow();
    let Some(mut work) = queue.first.take() else {
        return;
    };
    queue.first = queue.rest.pop_front();
    drop(queue);
    #[cfg(feature = "thread-trace")]
    let start = crate::arch::platform::timestamp();
    EXECUTING[slot].store(true, Ordering::Release);
    let outcome = work.thread.step_on(slot, &work.table);
    EXECUTING[slot].store(false, Ordering::Release);
    *work.result.0.borrow() = Some(Returned {
        thread: work.thread,
        outcome,
        #[cfg(feature = "thread-trace")]
        interval: (slot, start, crate::arch::platform::timestamp()),
    });
    work.waker.wake();
}
