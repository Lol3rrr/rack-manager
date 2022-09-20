pub trait StaticList<C> {
    fn length(&self) -> usize;

    fn get_task<'s, 'p>(&'s self, index: usize) -> Option<&'p dyn StaticList<C>>
    where
        's: 'p;

    fn get_task_mut<'s, 'p>(&'s mut self, index: usize) -> Option<&'p mut dyn StaticList<C>>
    where
        's: 'p;

    fn content<'s>(&'s mut self) -> Option<&'s mut C>;
}

pub struct StaticListEnd {}

impl<C> StaticList<C> for StaticListEnd {
    fn length(&self) -> usize {
        0
    }

    fn get_task<'s, 'p>(&'s self, _: usize) -> Option<&'p dyn StaticList<C>>
    where
        's: 'p,
    {
        None
    }

    fn get_task_mut<'s, 'p>(&'s mut self, _: usize) -> Option<&'p mut dyn StaticList<C>>
    where
        's: 'p,
    {
        None
    }

    fn content<'s>(&'s mut self) -> Option<&'s mut C> {
        None
    }
}
