/// A static List, is a List of Elements, which is entirely known at compile Time and therefore
/// does not require any memory allocations, but also does not allow for adding/removing Items.
pub trait StaticList<C> {
    /// The Length of the List
    fn length(&self) -> usize;

    /// Attempts to load the List starting at the given Index
    fn get<'s, 'p>(&'s self, index: usize) -> Option<&'p dyn StaticList<C>>
    where
        's: 'p;

    /// Attempts to get a mutable reference to the List starting at the given Index
    fn get_mut<'s, 'p>(&'s mut self, index: usize) -> Option<&'p mut dyn StaticList<C>>
    where
        's: 'p;

    /// Get the Content of the current starting Node of the List
    fn content<'s>(&'s mut self) -> Option<&'s mut C>;
}

/// An End-Marker for a Static List
pub struct StaticListEnd {}

impl<C> StaticList<C> for StaticListEnd {
    fn length(&self) -> usize {
        0
    }

    fn get<'s, 'p>(&'s self, _: usize) -> Option<&'p dyn StaticList<C>>
    where
        's: 'p,
    {
        None
    }

    fn get_mut<'s, 'p>(&'s mut self, _: usize) -> Option<&'p mut dyn StaticList<C>>
    where
        's: 'p,
    {
        None
    }

    fn content<'s>(&'s mut self) -> Option<&'s mut C> {
        None
    }
}
