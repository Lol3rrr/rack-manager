use core::future::Future;

/// Allows for Yielding from the current async Task once, but still being marked as executable
/// immediately. This is useful for making sure that an async Task yields at least once, when it
/// hits this Future and allows other Futures to be run
pub struct YieldNow {
    polled: bool,
}

impl YieldNow {
    pub fn new() -> Self {
        YieldNow { polled: false }
    }
}

impl Default for YieldNow {
    fn default() -> Self {
        Self::new()
    }
}

pub fn yield_now() -> YieldNow {
    YieldNow::new()
}

impl Future for YieldNow {
    type Output = ();

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        if self.polled {
            core::task::Poll::Ready(())
        } else {
            self.polled = true;
            cx.waker().wake_by_ref();

            core::task::Poll::Pending
        }
    }
}
