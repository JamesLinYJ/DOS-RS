//! POSIX advisory byte-range locks follow process and file identity, including hard links.
//! The scheduler services requests on the boot CPU after all threads release user memory.
use super::{Description, Handle, ProcessContext};
use crate::{arch::user::AddressSpace, kernel_cell::KernelCell, posix::executable::errno};
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use core::task::{Context, Poll, Waker};
use dosrs_native_posix::Process;

type Key = (u32, u64);
const TO_EOF: u64 = 1u64 << 63;
const READ: i16 = 0;
const WRITE: i16 = 1;
const UNLOCK: i16 = 2;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Process(u32),
    Description(usize),
}
impl Owner {
    fn reported_pid(self) -> i32 {
        match self {
            Self::Process(pid) => pid as i32,
            Self::Description(_) => -1,
        }
    }
}
#[derive(Clone, Copy)]
struct Lock {
    key: Key,
    owner: Owner,
    kind: i16,
    first: u64,
    end: u64,
}
impl Lock {
    fn overlaps(self, other: Self) -> bool {
        self.key == other.key && self.first < other.end && other.first < self.end
    }
    fn blocks(self, other: Self) -> bool {
        self.owner != other.owner
            && self.overlaps(other)
            && (self.kind == WRITE || other.kind == WRITE)
    }
}
struct Waiting {
    requested: Lock,
    waker: Weak<Waker>,
}
struct Table {
    owners: Vec<Lock>,
    waiting: Vec<Waiting>,
}
static TABLE: KernelCell<Table> = KernelCell::new(Table {
    owners: Vec::new(),
    waiting: Vec::new(),
});
impl Table {
    fn wake(&mut self) {
        for waiter in self.waiting.drain(..) {
            if let Some(w) = waiter.waker.upgrade() {
                w.wake_by_ref();
            }
        }
    }
    fn wait(&mut self, wanted: Lock, waker: &Arc<Waker>) -> Result<(), isize> {
        self.waiting.retain(|entry| entry.waker.strong_count() != 0);
        // Follow owners that are themselves suspended on a conflicting range.
        let mut chain = Vec::new();
        chain.try_reserve(self.owners.len()).map_err(|_| -37isize)?;
        let extend = |request: Lock, chain: &mut Vec<Owner>| {
            for owner in self.owners.iter().filter(|lock| lock.blocks(request)) {
                if !chain.contains(&owner.owner) {
                    chain.push(owner.owner);
                }
            }
        };
        extend(wanted, &mut chain);
        let mut i = 0;
        while i < chain.len() {
            let pid = chain[i];
            if pid == wanted.owner {
                return Err(-35);
            }
            for w in self.waiting.iter().filter(|w| w.requested.owner == pid) {
                extend(w.requested, &mut chain);
            }
            i += 1;
        }
        let weak = Arc::downgrade(waker);
        if !self.waiting.iter().any(|w| Weak::ptr_eq(&w.waker, &weak)) {
            self.waiting.try_reserve(1).map_err(|_| -37isize)?;
            self.waiting.push(Waiting {
                requested: wanted,
                waker: weak,
            });
        }
        Ok(())
    }
    fn change(&mut self, wanted: Lock) -> Result<(), isize> {
        // One replacement can split one existing interval into two and add a new lock.
        self.owners.try_reserve(2).map_err(|_| -37isize)?;
        let mut i = 0;
        while i < self.owners.len() {
            let old = self.owners[i];
            if old.owner != wanted.owner || !old.overlaps(wanted) {
                i += 1;
                continue;
            }
            self.owners.swap_remove(i);
            if old.first < wanted.first {
                self.owners.push(Lock {
                    end: wanted.first,
                    ..old
                });
            }
            if old.end > wanted.end {
                self.owners.push(Lock {
                    first: wanted.end,
                    ..old
                });
            }
        }
        if wanted.kind != UNLOCK {
            let mut merged = wanted;
            self.owners.retain(|old| {
                if (old.owner, old.key, old.kind) == (wanted.owner, wanted.key, wanted.kind)
                    && (old.end == wanted.first || old.first == wanted.end)
                {
                    merged.first = merged.first.min(old.first);
                    merged.end = merged.end.max(old.end);
                    false
                } else {
                    true
                }
            });
            self.owners.push(merged);
        }
        self.wake();
        Ok(())
    }
}
pub(super) fn release(pid: u32, key: Option<Key>) {
    release_owner(Owner::Process(pid), key);
}
pub(super) fn release_description(id: usize) {
    release_owner(Owner::Description(id), None);
}
fn release_owner(owner: Owner, key: Option<Key>) {
    let mut t = TABLE.borrow();
    let old = t.owners.len();
    t.owners
        .retain(|l| l.owner != owner || key.is_some_and(|k| k != l.key));
    if t.owners.len() != old {
        t.wake();
    }
}
pub(in crate::posix) struct Request {
    wanted: Lock,
    handle: Handle,
    waker: Arc<Waker>,
}

fn read_lock(
    p: &ProcessContext,
    image: &AddressSpace,
    fd: usize,
    cmd: usize,
    addr: usize,
    cx: &Context,
) -> Result<Request, isize> {
    let mut buf = [0; 32];
    image.read(addr, &mut buf).map_err(|_| -14isize)?;
    let kind = i16::from_le_bytes(buf[0..2].try_into().unwrap());
    let whence = i16::from_le_bytes(buf[2..4].try_into().unwrap());
    let start = i64::from_le_bytes(buf[8..16].try_into().unwrap());
    let len = i64::from_le_bytes(buf[16..24].try_into().unwrap());
    if cmd >= 36 && buf[24..28] != [0; 4] {
        return Err(-22);
    }
    if !matches!(kind, READ | WRITE | UNLOCK) || (matches!(cmd, 5 | 36) && kind == UNLOCK) {
        return Err(-22);
    }
    let handle = p.get(fd).map_err(errno)?.clone();
    let (base, key) = match &*handle.borrow() {
        Description::File {
            object,
            offset,
            access,
            ..
        } => {
            if !matches!(cmd, 5 | 36)
                && (kind == READ && !access.readable() || kind == WRITE && !access.writable())
            {
                return Err(-9);
            }
            let base = match whence {
                0 => 0,
                1 => *offset,
                2 => object.len().map_err(errno)?,
                _ => return Err(-22),
            };
            (base, object.identity())
        }
        _ => return Err(-9),
    };
    let first = base as i128 + start as i128;
    let next = if len == 0 {
        TO_EOF as i128
    } else {
        first + len as i128
    };
    if first < 0 || next < 0 {
        return Err(-22);
    }
    if first >= TO_EOF as i128 || next > TO_EOF as i128 {
        return Err(-75);
    }
    Ok(Request {
        wanted: Lock {
            key,
            owner: if cmd >= 36 {
                Owner::Description(Arc::as_ptr(&handle) as usize)
            } else {
                Owner::Process(p.getpid())
            },
            kind,
            first: first.min(next) as u64,
            end: first.max(next) as u64,
        },
        handle,
        waker: Arc::new(cx.waker().clone()),
    })
}
pub(in crate::posix) fn call(
    slot: &mut Option<Request>,
    p: &ProcessContext,
    image: &mut AddressSpace,
    [fd, cmd, addr]: [usize; 3],
    cx: &Context,
) -> Poll<Result<usize, isize>> {
    if slot.is_none() {
        *slot = Some(read_lock(p, image, fd, cmd, addr, cx)?);
    }
    let r = slot.as_ref().unwrap();
    if !Arc::ptr_eq(p.get(fd).map_err(errno)?, &r.handle) {
        return Poll::Ready(Err(-9));
    }
    let mut t = TABLE.borrow();
    let conflict = t
        .owners
        .iter()
        .filter(|o| o.blocks(r.wanted))
        .min_by_key(|o| o.first)
        .copied();
    if matches!(cmd, 5 | 36) {
        if let Some(l) = conflict {
            let mut out = [0; 32];
            out[..2].copy_from_slice(&l.kind.to_le_bytes());
            out[8..16].copy_from_slice(&(l.first as i64).to_le_bytes());
            let len = if l.end == TO_EOF {
                0
            } else {
                (l.end - l.first) as i64
            };
            out[16..24].copy_from_slice(&len.to_le_bytes());
            out[24..28].copy_from_slice(&l.owner.reported_pid().to_le_bytes());
            image.write(addr, &out).map_err(|_| -14isize)?;
        } else {
            image
                .write(addr, &UNLOCK.to_le_bytes())
                .map_err(|_| -14isize)?;
        }
        return Poll::Ready(Ok(0));
    }
    if r.wanted.kind != UNLOCK && conflict.is_some() {
        if matches!(cmd, 6 | 37) {
            return Poll::Ready(Err(-11));
        }
        t.wait(r.wanted, &r.waker)?;
        return Poll::Pending;
    }
    t.change(r.wanted)?;
    Poll::Ready(Ok(0))
}
