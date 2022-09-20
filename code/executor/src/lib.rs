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

mod helper;
pub use helper::YieldNow;

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

    pub fn run(mut self) -> ! {
        loop {
            for (id, (entry, iwaker)) in
                self.metadata.iter_mut().zip(self.wakers.iter()).enumerate()
            {
                if !iwaker.is_ready() || entry.done {
                    continue;
                }
                iwaker.set_ready(false);

                let task = self.tasks.get_task_mut(id).unwrap();
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
