//! A no_std compatible async executor, without any dynamic memory allocation.
//!
//! # Scope
//! This crate is only intended to provide the executor and nothing else. So most extra features
//! that you would expect, like Timers, etc., are not included with or intended for this crate
//! and instead need to be provided by another external crate.
//!
//! # Example
//! ```rust,no_run
//! # use executor::{tasks, Runtime};
//! async fn first() {}
//! async fn second() {}
//!
//! tasks!(list, (first(), first_task), (second(), second_task));
//!
//! let runtime = Runtime::new(list);
//! runtime.run();
//! ```
#![cfg_attr(not(test), no_std)]
#![feature(generic_const_exprs)]

use core::{
    array,
    task::{Context, Poll},
};

mod staticlist;
pub use staticlist::StaticList;

mod tasklist;
pub use tasklist::*;

mod waking;

/// An async Runtime for a no_std environment, which does not perform any dynamic memory allocation.
///
/// This runtime only handles a fixed number of async Tasks, that are known at compile-time and
/// does not support dynamically starting/spawning new Tasks.
pub struct Runtime<'f, T, const L: usize> {
    metadata: [TaskMetadata; L],
    wakers: [waking::InternalWaker; L],
    tasks: Task<'f, T, L>,
}

struct TaskMetadata {
    done: bool,
    id: usize,
}

impl<'f, T, const L: usize> Runtime<'f, T, L>
where
    T: TaskList<'f>,
{
    /// Creates a new Runtime for the List of Tasks
    pub fn new(tasks: Task<'f, T, L>) -> Self {
        let wakers = array::from_fn(|_| waking::InternalWaker::new());
        let meta = array::from_fn(|idx| TaskMetadata {
            done: false,
            id: idx,
        });

        Self {
            tasks,
            wakers,
            metadata: meta,
        }
    }

    /// Actually starts/runs the Runtime, this will never return as we expect the Tasks to run
    /// forever.
    pub fn run(mut self) -> ! {
        loop {
            for (id, (entry, iwaker)) in
                self.metadata.iter_mut().zip(self.wakers.iter()).enumerate()
            {
                if !iwaker.is_ready() || entry.done {
                    continue;
                }
                iwaker.set_ready(false);

                let task = self.tasks.get_mut(id).unwrap();
                let task_fut = task.content().unwrap();

                let waker = unsafe { waking::create_waker(iwaker) };
                let mut context = Context::from_waker(&waker);

                match task_fut.as_mut().poll(&mut context) {
                    Poll::Pending => {}
                    Poll::Ready(_) => {
                        entry.done = true;
                    }
                };
            }

            assert!(self.metadata.iter().any(|m| !m.done), "Should run forever");
        }
    }
}
