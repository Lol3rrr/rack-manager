use core::future::Future;

pub struct YieldNow {
    polled: bool,
}

impl YieldNow {
    pub fn new() -> Self {
        YieldNow { polled: false }
    }
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
