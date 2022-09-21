use core::{future::Future, pin::Pin};

use crate::staticlist::{StaticList, StaticListEnd};

/// Allows to more easily construct a TaskList
#[macro_export]
macro_rules! tasks {
    ($name:ident, ($fut:expr, $fut_n:ident), $(($futs:expr, $futs_n:ident)),*) => {
        let mut $fut_n = $fut;
        $(
            let mut $futs_n = $futs;
        )*

        let $name = $crate::Task::new(&mut $fut_n);
        $(
            let $name = $name.append($crate::Task::new(&mut $futs_n));
        )*
    };
}

/// Generalises over a Static List of Tasks to be executed by the Runtime
pub trait TaskList<'f>: StaticList<Pin<&'f mut dyn Future<Output = ()>>> {}

/// A single Task-Node in the TaskList
pub struct Task<'f, N, const L: usize> {
    fut: Pin<&'f mut dyn Future<Output = ()>>,
    next: Option<N>,
}

impl<'f> Task<'f, StaticListEnd, 1> {
    /// Creates a single Node List
    pub fn new(fut: &'f mut dyn Future<Output = ()>) -> Self {
        Self {
            fut: unsafe { Pin::new_unchecked(fut) },
            next: None,
        }
    }
}
impl<'f, N, const L: usize> Task<'f, N, L> {
    /// Appends self to the given Node and returns the new starting Node of the resulting List
    pub fn append<'af>(
        self,
        append: Task<'af, StaticListEnd, 1>,
    ) -> Task<'af, Task<'f, N, L>, { L + 1 }> {
        Task {
            fut: append.fut,
            next: Some(self),
        }
    }
}

impl<'f, N, const L: usize> StaticList<Pin<&'f mut dyn Future<Output = ()>>> for Task<'f, N, L>
where
    N: StaticList<Pin<&'f mut dyn Future<Output = ()>>>,
{
    fn length(&self) -> usize {
        L
    }

    fn get<'s, 'p>(
        &'s self,
        index: usize,
    ) -> Option<&'p dyn StaticList<Pin<&'f mut dyn Future<Output = ()>>>>
    where
        's: 'p,
    {
        if index == 0 {
            Some(self)
        } else {
            self.next.as_ref().and_then(|n| n.get(index - 1))
        }
    }

    fn get_mut<'s, 'p>(
        &'s mut self,
        index: usize,
    ) -> Option<&'p mut dyn StaticList<Pin<&'f mut dyn Future<Output = ()>>>>
    where
        's: 'p,
    {
        if index == 0 {
            Some(self)
        } else {
            self.next.as_mut().and_then(|n| n.get_mut(index - 1))
        }
    }

    fn content<'s>(&'s mut self) -> Option<&'s mut Pin<&'f mut dyn Future<Output = ()>>> {
        Some(&mut self.fut)
    }
}

impl<'f, N, const L: usize> TaskList<'f> for Task<'f, N, L> where
    N: StaticList<Pin<&'f mut dyn Future<Output = ()>>>
{
}
impl<'f> TaskList<'f> for StaticListEnd {}
