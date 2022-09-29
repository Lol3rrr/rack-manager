#![cfg_attr(not(test), no_std)]
#![feature(allocator_api)]
#![feature(ptr_sub_ptr)]
#![feature(nonnull_slice_from_raw_parts)]

#[cfg(feature = "stm32l432")]
mod stm32l432;
#[cfg(feature = "stm32l432")]
pub use stm32l432::*;

pub mod queue;

pub mod allocator;

pub mod futures;

pub mod timer;

pub(crate) mod atomic;

#[cfg(not(loom))]
mod unsafecell {
    pub(crate) struct UnsafeCell<T>(core::cell::UnsafeCell<T>);

    impl<T> UnsafeCell<T> {
        pub(crate) const fn new(data: T) -> UnsafeCell<T> {
            UnsafeCell(core::cell::UnsafeCell::new(data))
        }

        pub(crate) fn with_mut<R>(&self, f: impl FnOnce(*mut T) -> R) -> R {
            f(self.0.get())
        }
    }
}
#[cfg(not(loom))]
pub(crate) use unsafecell::*;

#[cfg(loom)]
pub(crate) use loom::cell::UnsafeCell;
