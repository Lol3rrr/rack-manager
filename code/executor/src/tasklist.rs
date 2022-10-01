use core::{future::Future, pin::Pin};

use crate::staticlist::{StaticList, StaticListEnd};

/// Allows to more easily construct a TaskList.
///
/// # Usage
/// The first identifier is the name for the variable that is created for your list, which is then
/// followed by a list of tuples consisting of (future, name), where future can be any expression
/// that evalutes to a future and the name is the name for a variable created for this future.
///
/// # Example
/// ```rust
/// # use executor::tasks;
/// async fn first() {}
/// async fn second() {}
///
/// // This will create a new variable named "list" that is your task-list containing the futures
/// // returned by first() and second()
/// tasks!(list, (first(), first_task), (second(), second_task));
/// ```
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

/// A single Task-Node in the TaskList.
///
/// # Usage
/// Although this can be used to construct a TaskList yourself, it is recommended to instead
/// use the [`tasks`] macro, which greatly simplifies the usage.
///
/// # Example - Building a list of Tasks
/// ```rust
/// # use executor::{Task, StaticList};
/// async fn first() {}
/// async fn second() {}
///
/// let mut first_task = first();
/// let mut second_task = second();
///
/// let list = Task::new(&mut first_task).append(Task::new(&mut second_task));
/// # assert_eq!(2, list.length());
/// ```
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
