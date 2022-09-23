use core::{alloc::Allocator, ptr::NonNull};

use crate::atomic::{self, AtomicPtr};

pub struct LinkedListAllocator<const N: usize> {
    head: AtomicPtr<u8>,
    start: *mut u8,
    end: *mut u8,
}

unsafe impl<const N: usize> Sync for LinkedListAllocator<N> {}

macro_rules! alloc_impl {
    ($size:expr) => {
        impl LinkedListAllocator<$size> {
            #[allow(clippy::not_unsafe_ptr_arg_deref)]
            pub fn new(start: *mut u8, end: *mut u8) -> Self {
                let last_ptr = unsafe { end.offset(-$size) };

                for offset in 1..(unsafe { last_ptr.sub_ptr(start) } / $size + 1) {
                    let next_ptr = unsafe { start.add(offset * $size) };
                    debug_assert!(next_ptr <= last_ptr);

                    let current_ptr =
                        (unsafe { start.add((offset - 1) * $size) }) as *mut *const u8;
                    unsafe {
                        core::ptr::write(current_ptr, next_ptr as *const u8);
                    }
                }

                unsafe {
                    core::ptr::write(last_ptr as *mut *const u8, core::ptr::null_mut());
                }

                Self {
                    head: AtomicPtr::new(start as *mut u8),
                    start,
                    end,
                }
            }
        }

        unsafe impl Allocator for LinkedListAllocator<$size> {
            fn allocate(
                &self,
                layout: core::alloc::Layout,
            ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
                if layout.align() <= $size && layout.size() <= $size {
                    loop {
                        let ptr = self.head.load(atomic::Ordering::SeqCst);
                        if ptr.is_null() {
                            panic!("No more memory to allocate")
                        }

                        let ptr_ref = unsafe { &*(ptr as *mut *mut u8) };

                        let follow_ptr = unsafe { core::ptr::read_volatile(ptr_ref) };

                        match self.head.compare_exchange(
                            ptr,
                            follow_ptr as *mut u8,
                            atomic::Ordering::SeqCst,
                            atomic::Ordering::SeqCst,
                        ) {
                            Ok(_) => {
                                return Ok(NonNull::slice_from_raw_parts(
                                    NonNull::new(ptr as *mut u8).unwrap(),
                                    $size,
                                ))
                            }
                            Err(_) => continue,
                        };
                    }
                } else {
                    let needed_blocks = if layout.size() % $size == 0 {
                        layout.size() / $size
                    } else {
                        (layout.size() / $size) + 1
                    };

                    todo!("Allocation too big, needs {} Blocks", needed_blocks)
                }
            }

            unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: core::alloc::Layout) {
                if layout.size() <= $size {
                    let ptr_block = ptr.as_ptr() as *mut *mut u8;

                    loop {
                        let current_head = self.head.load(atomic::Ordering::SeqCst);

                        unsafe {
                            core::ptr::write_volatile(ptr_block, current_head);
                        }

                        if self
                            .head
                            .compare_exchange(
                                current_head,
                                ptr_block as *mut u8,
                                atomic::Ordering::SeqCst,
                                atomic::Ordering::SeqCst,
                            )
                            .is_ok()
                        {
                            break;
                        }
                    }
                } else {
                    todo!("Deallocate multiple Blocks")
                }
            }
        }
    };
}

alloc_impl!(64);
alloc_impl!(128);
alloc_impl!(256);
alloc_impl!(512);
alloc_impl!(1024);

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;

    #[test]
    fn create_alloc() {
        let mut buffer: Vec<u8> = vec![0; 1024];
        let ptr = buffer.as_mut_ptr_range();

        let allocator = LinkedListAllocator::<256>::new(ptr.start, ptr.end);
    }

    #[test]
    fn alloc_dealloc_box() {
        let mut buffer: Vec<u8> = vec![0; 1024];
        let ptr = buffer.as_mut_ptr_range();

        let allocator = LinkedListAllocator::<256>::new(ptr.start, ptr.end);

        let boxed = Box::new_in(13, &allocator);
        drop(boxed);
    }

    #[test]
    fn exhaustive_allocations() {
        let mut buffer: Vec<u8> = vec![0; 1024];
        let ptr = buffer.as_mut_ptr_range();

        let allocator = LinkedListAllocator::<256>::new(ptr.start, ptr.end);

        let boxed1 = Box::new_in(13, &allocator);
        let boxed2 = Box::new_in(13, &allocator);
        let boxed3 = Box::new_in(13, &allocator);
        let boxed4 = Box::new_in(13, &allocator);

        drop(boxed4);
        drop(boxed3);
        drop(boxed2);
        drop(boxed1);
    }

    #[test]
    #[should_panic]
    fn over_allocation() {
        let mut buffer: Vec<u8> = vec![0; 1024];
        let ptr = buffer.as_mut_ptr_range();

        let allocator = LinkedListAllocator::<256>::new(ptr.start, ptr.end);

        let boxed1 = Box::new_in(13, &allocator);
        let boxed2 = Box::new_in(13, &allocator);
        let boxed3 = Box::new_in(13, &allocator);
        let boxed4 = Box::new_in(13, &allocator);

        let boxed_over = Box::new_in(13, &allocator);
        drop(boxed_over);

        drop(boxed4);
        drop(boxed3);
        drop(boxed2);
        drop(boxed1);
    }

    #[test]
    fn big_allocation_double() {
        let mut buffer: Vec<u8> = vec![0; 1024];
        let ptr = buffer.as_mut_ptr_range();

        let allocator = LinkedListAllocator::<256>::new(ptr.start, ptr.end);

        let test: Vec<u8, &LinkedListAllocator<256>> = Vec::with_capacity_in(300, &allocator);
        drop(test);
    }
}

#[cfg(all(test, loom))]
mod loom_tests {
    use super::*;

    use loom::sync::Arc;

    #[test]
    fn allocs() {
        loom::model(|| {
            let mut buffer: Vec<u8> = vec![0; 1024];
            let ptr = buffer.as_mut_ptr_range();

            let allocator1 = Arc::new(LinkedListAllocator::<256>::new(ptr.start, ptr.end));
            let allocator2 = allocator1.clone();

            loom::thread::spawn(move || {
                let boxed = Box::new_in(13, &*allocator1);
                drop(boxed);
            });
            loom::thread::spawn(move || {
                let boxed = Box::new_in(13, &*allocator2);
                drop(boxed);
            });
        });
    }
}
