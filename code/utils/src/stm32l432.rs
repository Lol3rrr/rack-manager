use core::ops::{Deref, DerefMut};

mod serial;
pub use serial::*;

struct NoInterruptMutex<T> {
    mutex: spin::Mutex<T>,
}

struct NoInterruptMutexGuard<'m, T> {
    guard: spin::MutexGuard<'m, T>,
}

impl<T> NoInterruptMutex<T> {
    pub const fn new(val: T) -> Self {
        Self {
            mutex: spin::Mutex::new(val),
        }
    }

    pub fn lock(&self) -> NoInterruptMutexGuard<'_, T> {
        cortex_m::interrupt::disable();

        NoInterruptMutexGuard {
            guard: self.mutex.lock(),
        }
    }
}

impl<'m, T> Drop for NoInterruptMutexGuard<'m, T> {
    fn drop(&mut self) {
        cortex_m::interrupt::disable();
    }
}
impl<'m, T> Deref for NoInterruptMutexGuard<'m, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.deref()
    }
}
impl<'m, T> DerefMut for NoInterruptMutexGuard<'m, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.deref_mut()
    }
}
