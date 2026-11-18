//! Readiness subscriptions never consume input or hold a task alive after a wait.
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use core::task::Waker;

pub(super) const READ: i16 = 0x001 | 0x040; // POLLIN, POLLRDNORM
pub(super) const WRITE: i16 = 0x004 | 0x100; // POLLOUT, POLLWRNORM
pub(super) const ERROR: i16 = 0x008;
pub(super) const HANGUP: i16 = 0x010;
pub(super) const INVALID: i16 = 0x020;

#[derive(Default)]
pub(super) struct Subscribers(Vec<Weak<Waker>>);

impl Subscribers {
    pub fn subscribe(&mut self, waker: Option<&Arc<Waker>>) -> Result<(), isize> {
        if let Some(waker) = waker {
            self.0.retain(|entry| entry.strong_count() != 0);
            let entry = Arc::downgrade(waker);
            if !self
                .0
                .iter()
                .any(|registered| Weak::ptr_eq(registered, &entry))
            {
                self.0.try_reserve(1).map_err(|_| -12isize)?;
                self.0.push(entry);
            }
        }
        Ok(())
    }

    pub fn wake(&mut self) {
        for entry in self.0.drain(..) {
            if let Some(waker) = entry.upgrade() {
                waker.wake_by_ref();
            }
        }
    }
}
