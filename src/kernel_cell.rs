//! Scoped exclusive access to storage that must live outside the boot stack.
//! Wait for other CPUs to finish short critical sections. Callers which can
//! make progress elsewhere use `try_borrow` and leave the owner running.
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct KernelCell<T> {
    borrowed: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: Only the unique guard accesses the value. Acquisition and release
// synchronize successive guards, including when host tests run on other threads.
unsafe impl<T: Send> Sync for KernelCell<T> {}

impl<T> KernelCell<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self {
            borrowed: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub(crate) fn try_borrow(&self) -> Option<Guard<'_, T>> {
        self.borrowed
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Guard {
                cell: self,
                exclusive: core::marker::PhantomData,
            })
    }

    pub(crate) fn borrow(&self) -> Guard<'_, T> {
        loop {
            if let Some(guard) = self.try_borrow() {
                return guard;
            }
            while self.borrowed.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
    }
}

pub(crate) struct Guard<'a, T> {
    cell: &'a KernelCell<T>,
    exclusive: core::marker::PhantomData<&'a mut T>,
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: The guard owns the borrow until Drop, and this reference
        // cannot outlive the guard or overlap a mutable borrow of the guard.
        unsafe { &*self.cell.value.get() }
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: The flag grants exclusive access and the guard is mutably borrowed.
        unsafe { &mut *self.cell.value.get() }
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.cell.borrowed.store(false, Ordering::Release);
    }
}
