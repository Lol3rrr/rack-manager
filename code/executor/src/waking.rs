use core::{
    sync::atomic,
    sync::atomic::AtomicBool,
    task::{RawWaker, RawWakerVTable, Waker},
};

static VTABLE: RawWakerVTable = RawWakerVTable::new(w_clone, w_wake, w_wake_by_ref, w_drop);

unsafe fn w_clone(ptr: *const ()) -> RawWaker {
    RawWaker::new(ptr, &VTABLE)
}
unsafe fn w_wake(ptr: *const ()) {
    let entry = unsafe { &*(ptr as *const InternalWaker) };
    entry.set_ready(true);
}
unsafe fn w_wake_by_ref(ptr: *const ()) {
    let entry = unsafe { &*(ptr as *const InternalWaker) };
    entry.set_ready(true);
}
unsafe fn w_drop(_: *const ()) {}

/// The provided `InternalWaker` needs to have a lifetime that is valid for at least the entire
/// Duration of the Runtime
pub unsafe fn create_waker(iwaker: &InternalWaker) -> Waker {
    let raw_waker = RawWaker::new(iwaker as *const InternalWaker as *const (), &VTABLE);
    unsafe { Waker::from_raw(raw_waker) }
}

pub struct InternalWaker {
    ready: AtomicBool,
}

impl InternalWaker {
    pub fn new() -> Self {
        Self {
            ready: AtomicBool::new(true),
        }
    }

    pub fn set_ready(&self, val: bool) {
        self.ready.store(val, atomic::Ordering::SeqCst);
    }
    pub fn is_ready(&self) -> bool {
        self.ready.load(atomic::Ordering::SeqCst)
    }
}
