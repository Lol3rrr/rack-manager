use core::ops::{Deref, DerefMut};

mod serial;
pub use serial::*;

pub mod logging;

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

    pub fn with_lock<F>(&self, func: F)
    where
        F: FnOnce(spin::MutexGuard<'_, T>),
    {
        cortex_m::interrupt::free(|_| {
            let guard = self.mutex.lock();
            func(guard);
        });
    }
}

impl<'m, T> Drop for NoInterruptMutexGuard<'m, T> {
    fn drop(&mut self) {
        unsafe {
            cortex_m::interrupt::enable();
        }
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
