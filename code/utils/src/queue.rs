mod bounded {
    mod mpsc {}
}

pub mod unbounded {
    use core::alloc::Allocator;

    pub mod mpsc {
        //! A simple unbounded mpsc queue.
        //!
        //! # Memory Usage
        //! This can easily leak memory, if the Receiver is dropped before all the Senders are dropped.

        use core::{alloc::Layout, ptr::NonNull};

        use crate::{
            atomic::{self, AtomicIsize, AtomicPtr, AtomicU8, AtomicUsize},
            UnsafeCell,
        };

        use super::Allocator;

        pub trait QueueRx<T> {
            type ReceiveError;
            fn try_dequeue(&mut self) -> Result<T, Self::ReceiveError>;
        }

        pub trait QueueTx<T> {
            type SendError;
            fn try_enqueue(&self, data: T) -> Result<(), (T, Self::SendError)>;
        }

        struct Entry<T> {
            data: UnsafeCell<Option<T>>,
            state: AtomicU8,
        }

        impl<T> Default for Entry<T> {
            fn default() -> Self {
                Self {
                    state: AtomicU8::new(0),
                    data: UnsafeCell::new(None),
                }
            }
        }

        struct Buffer<T, const N: usize> {
            entries: [Entry<T>; N],
            pos: AtomicUsize,
            ref_count: AtomicIsize,
            next: AtomicPtr<Self>,
        }

        pub struct Tx<'a, T, A>
        where
            A: Allocator,
        {
            allocator: &'a A,
            tail: AtomicPtr<Buffer<T, 4>>,
        }
        pub struct Rx<'a, T, A>
        where
            A: Allocator,
        {
            allocator: &'a A,
            head: *const Buffer<T, 4>,
            pos: usize,
        }

        pub fn queue<T, A>(allocator: &A) -> (Tx<'_, T, A>, Rx<'_, T, A>)
        where
            A: Allocator,
        {
            let buffer = Buffer::allocate(allocator);
            unsafe { &*buffer }
                .ref_count
                .fetch_add(1, atomic::Ordering::SeqCst);

            (
                Tx {
                    allocator,
                    tail: AtomicPtr::new(buffer),
                },
                Rx {
                    allocator,
                    head: buffer,
                    pos: 0,
                },
            )
        }

        impl<T, const N: usize> Buffer<T, N> {
            pub fn allocate<A>(allocator: &A) -> *mut Self
            where
                A: Allocator,
            {
                let buffer: NonNull<Buffer<T, N>> = NonNull::new(
                    allocator
                        .allocate(Layout::new::<Buffer<T, N>>())
                        .unwrap()
                        .as_ptr() as *mut Buffer<T, N>,
                )
                .unwrap();
                unsafe {
                    core::ptr::write(
                        buffer.as_ptr(),
                        Buffer {
                            entries: core::array::from_fn(|_| Entry::default()),
                            pos: AtomicUsize::new(0),
                            ref_count: AtomicIsize::new(0),
                            next: AtomicPtr::default(),
                        },
                    );
                }

                buffer.as_ptr()
            }

            pub fn try_enqueue(&self, data: T) -> Result<(), T> {
                let insert_pos = self.pos.fetch_add(1, atomic::Ordering::SeqCst);
                if insert_pos >= N {
                    return Err(data);
                }

                let entry = &self.entries[insert_pos];

                entry.state.store(1, atomic::Ordering::SeqCst);

                entry.data.with_mut(|data_ptr| unsafe {
                    core::ptr::write(data_ptr, Some(data));
                });

                entry.state.store(2, atomic::Ordering::SeqCst);

                Ok(())
            }
        }

        #[derive(Debug, PartialEq, Eq)]
        pub enum DequeueError {
            Empty,
        }

        impl<'a, T, A> Tx<'a, T, A>
        where
            A: Allocator,
        {
            pub fn try_enqueue(&self, mut data: T) {
                loop {
                    let tail_ptr = self.tail.load(atomic::Ordering::SeqCst);
                    let buffer = unsafe { &*tail_ptr };

                    match buffer.try_enqueue(data) {
                        Ok(_) => return,
                        Err(d) => {
                            data = d;

                            let current_next = match buffer.next.load(atomic::Ordering::SeqCst) {
                                ptr if ptr.is_null() => {
                                    let new_buffer = Buffer::allocate(self.allocator);

                                    match buffer.next.compare_exchange(
                                        core::ptr::null_mut(),
                                        new_buffer,
                                        atomic::Ordering::SeqCst,
                                        atomic::Ordering::SeqCst,
                                    ) {
                                        Ok(_) => new_buffer,
                                        Err(new_buffer) => new_buffer,
                                    }
                                }
                                ptr => ptr,
                            };

                            let next = unsafe { &*current_next };
                            next.ref_count.fetch_add(1, atomic::Ordering::SeqCst);

                            match self.tail.compare_exchange(
                                tail_ptr,
                                current_next,
                                atomic::Ordering::SeqCst,
                                atomic::Ordering::SeqCst,
                            ) {
                                Ok(_) => {}
                                Err(_) => {
                                    next.ref_count.fetch_sub(1, atomic::Ordering::SeqCst);
                                }
                            };

                            buffer.ref_count.fetch_sub(1, atomic::Ordering::SeqCst);
                        }
                    };
                }
            }
        }

        impl<'a, T, A> Drop for Tx<'a, T, A>
        where
            A: Allocator,
        {
            fn drop(&mut self) {
                let buffer = unsafe { &*self.tail.load(atomic::Ordering::SeqCst) };
                buffer.ref_count.fetch_sub(1, atomic::Ordering::SeqCst);
            }
        }

        impl<'a, T, A> Rx<'a, T, A>
        where
            A: Allocator,
        {
            pub fn try_dequeue(&mut self) -> Result<T, DequeueError> {
                let mut buf_ptr = self.head;
                let mut buffer = unsafe { &*buf_ptr };

                let mut initial = true;
                let mut possible_entries = &buffer.entries[self.pos..];
                loop {
                    let mut set_entries = possible_entries
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| e.state.load(atomic::Ordering::SeqCst) == 2);

                    match set_entries.next() {
                        Some((index, entry)) => {
                            let data = entry
                                .data
                                .with_mut(|data_ptr| unsafe { (*data_ptr).take() }.unwrap());

                            entry.state.store(3, atomic::Ordering::SeqCst);

                            if initial && self.pos == index {
                                self.pos += 1;
                            }

                            return Ok(data);
                        }
                        None => {
                            match buffer.next.load(atomic::Ordering::SeqCst) {
                                ptr if ptr.is_null() => return Err(DequeueError::Empty),
                                ptr => {
                                    if buffer
                                        .entries
                                        .iter()
                                        .all(|e| e.state.load(atomic::Ordering::SeqCst) == 3)
                                    {
                                        if buffer.ref_count.load(atomic::Ordering::SeqCst) == 0 {
                                            if self.head == buf_ptr && initial {
                                                self.head = ptr;
                                                self.pos = 0;

                                                unsafe {
                                                    self.allocator.deallocate(
                                                        NonNull::new(buf_ptr as *mut u8).unwrap(),
                                                        Layout::new::<Buffer<T, 4>>(),
                                                    );
                                                }
                                            }
                                        } else {
                                            todo!("Cant Free")
                                        }
                                    }

                                    buf_ptr = ptr;
                                    buffer = unsafe { &*buf_ptr };
                                    possible_entries = &buffer.entries;
                                    initial = false;
                                    continue;
                                }
                            };
                        }
                    };
                }
            }
        }

        impl<'a, T, A> Drop for Rx<'a, T, A>
        where
            A: Allocator,
        {
            fn drop(&mut self) {
                while !self.head.is_null() {
                    let buffer = unsafe { &*self.head };
                    if buffer.ref_count.load(atomic::Ordering::SeqCst) != 0 {
                        return;
                    }

                    for entry in buffer
                        .entries
                        .iter()
                        .filter(|e| e.state.load(atomic::Ordering::SeqCst) == 2)
                    {
                        let data = entry.data.with_mut(|data| unsafe { (*data).take() });
                        drop(data);
                    }

                    let current = self.head;

                    self.head = buffer.next.load(atomic::Ordering::SeqCst);

                    unsafe {
                        self.allocator.deallocate(
                            NonNull::new(current as *mut u8).unwrap(),
                            Layout::new::<Buffer<T, 4>>(),
                        );
                    }
                }
            }
        }

        impl<'a, T, A> QueueTx<T> for Tx<'a, T, A>
        where
            A: Allocator,
        {
            type SendError = ();

            fn try_enqueue(&self, data: T) -> Result<(), (T, Self::SendError)> {
                Tx::try_enqueue(self, data);
                Ok(())
            }
        }
        impl<'a, T, A> QueueRx<T> for Rx<'a, T, A>
        where
            A: Allocator,
        {
            type ReceiveError = DequeueError;

            fn try_dequeue(&mut self) -> Result<T, Self::ReceiveError> {
                Rx::try_dequeue(self)
            }
        }

        #[cfg(all(test, not(loom)))]
        mod tests {
            use super::*;

            #[test]
            fn test() {
                assert_eq!(72, core::mem::size_of::<Buffer<u8, 4>>());
                assert_eq!(72, core::mem::size_of::<Buffer<(u64, u64), 4>>());
            }

            #[test]
            fn create_queue() {
                let (tx, rx) = queue::<u8, _>(&std::alloc::System);

                drop(tx);
                drop(rx);
            }

            #[test]
            fn enqueue() {
                let (tx, rx) = queue(&std::alloc::System);

                tx.try_enqueue(13);

                drop(tx);
                drop(rx);
            }

            #[test]
            fn enqueue_more_buffers() {
                let (tx, rx) = queue(&std::alloc::System);

                for i in 0..100 {
                    tx.try_enqueue(i);
                }

                drop(tx);
                drop(rx);
            }

            #[test]
            fn dequeue_empty() {
                let (tx, mut rx) = queue::<u8, _>(&std::alloc::System);

                assert_eq!(rx.try_dequeue(), Err(DequeueError::Empty));

                drop(tx);
                drop(rx);
            }

            #[test]
            fn enqueue_dequeue() {
                let (tx, mut rx) = queue(&std::alloc::System);

                tx.try_enqueue(13);

                let res = rx.try_dequeue().unwrap();
                assert_eq!(13, res);

                drop(tx);
                drop(rx);
            }

            #[test]
            fn enqueue_dequeue_buffers() {
                let (tx, mut rx) = queue(&std::alloc::System);

                for i in 0..100 {
                    tx.try_enqueue(i);
                }

                for i in 0..100 {
                    assert_eq!(Ok(i), rx.try_dequeue());
                }

                drop(tx);
                drop(rx);
            }
        }

        #[cfg(all(test, loom))]
        mod loom_tests {
            use super::*;

            use loom::sync::Arc;

            #[test]
            fn two_enqueue_one_dequeue() {
                let mut model = loom::model::Builder::new();
                model.max_branches = 1000;

                model.check(|| {
                    let (rtx, mut rx) = queue::<u8, _>(&std::alloc::System);
                    let tx1 = Arc::new(rtx);
                    let tx2 = tx1.clone();

                    loom::thread::spawn(move || {
                        for i in 0..40 {
                            tx1.try_enqueue(i);
                        }
                    });
                    loom::thread::spawn(move || {
                        for i in 0..40 {
                            tx2.try_enqueue(i);
                        }
                    });

                    loom::thread::spawn(move || {
                        for _ in 0..80 {
                            // rx.try_dequeue();
                        }
                    });
                });
            }
        }
    }
}
