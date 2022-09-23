use core::future::Future;

use general::AsyncSerial;

use crate::{
    atomic::{self, AtomicU32},
    futures::yield_now,
    queue::unbounded::mpsc::{QueueRx, QueueTx},
};

pub struct SerialLoggerFrontend<T> {
    id: AtomicU32,
    tx: T,
}

impl<T> tracing_core::Subscriber for SerialLoggerFrontend<T>
where
    T: QueueTx<Message> + 'static,
{
    fn enabled(&self, metadata: &tracing_core::Metadata<'_>) -> bool {
        true
    }

    fn enter(&self, span: &tracing_core::span::Id) {
        self.tx.try_enqueue(Message::Enter(span.clone()));
    }

    fn event(&self, event: &tracing_core::Event<'_>) {
        self.tx.try_enqueue(Message::Event);
    }

    fn exit(&self, span: &tracing_core::span::Id) {
        self.tx.try_enqueue(Message::Exit(span.clone()));
    }

    fn new_span(&self, span: &tracing_core::span::Attributes<'_>) -> tracing_core::span::Id {
        let raw_id = self.id.fetch_add(1, atomic::Ordering::SeqCst);

        let n_id = tracing_core::span::Id::from_u64(raw_id as u64);

        self.tx.try_enqueue(Message::NewSpan(n_id.clone()));

        n_id
    }

    fn record(&self, span: &tracing_core::span::Id, values: &tracing_core::span::Record<'_>) {
        self.tx.try_enqueue(Message::Record(span.clone()));
    }

    fn record_follows_from(&self, span: &tracing_core::span::Id, follows: &tracing_core::span::Id) {
        // todo!()
    }

    fn try_close(&self, id: tracing_core::span::Id) -> bool {
        // todo!()
        false
    }

    fn clone_span(&self, id: &tracing_core::span::Id) -> tracing_core::span::Id {
        // todo!()
        id.clone()
    }
}

pub enum Message {
    NewSpan(tracing_core::span::Id),
    Enter(tracing_core::span::Id),
    Exit(tracing_core::span::Id),
    Record(tracing_core::span::Id),
    Event,
}

pub fn logger<S, R, T>(
    serial: S,
    rx: R,
    tx: T,
) -> (SerialLoggerFrontend<T>, impl Future<Output = ()>)
where
    S: AsyncSerial<256>,
    R: QueueRx<Message>,
    T: QueueTx<Message> + 'static,
{
    (
        SerialLoggerFrontend {
            id: AtomicU32::new(1),
            tx,
        },
        run_backend(rx, serial),
    )
}

async fn run_backend<R, S>(mut rx: R, mut serial: S)
where
    R: QueueRx<Message>,
    S: AsyncSerial<256>,
{
    let initial = "Starting Logging";
    let mut buffer = [0; 256];
    buffer[0..initial.len()].copy_from_slice(initial.as_bytes());
    serial.write(buffer).await;

    loop {
        yield_now().await;

        let msg = match rx.try_dequeue() {
            Ok(m) => m,
            Err(_) => {
                continue;
            }
        };

        match msg {
            Message::NewSpan(id) => {
                let mut buffer = [0; 256];

                let span_beginning = "New-Span ";
                buffer[0..span_beginning.len()].copy_from_slice(span_beginning.as_bytes());

                serial.write(buffer);
            }
            Message::Enter(id) => {
                let mut buffer = [0; 256];

                let span_beginning = "Enter ";
                buffer[0..span_beginning.len()].copy_from_slice(span_beginning.as_bytes());

                serial.write(buffer);
            }
            Message::Exit(id) => {
                let mut buffer = [0; 256];

                let span_beginning = "Exit ";
                buffer[0..span_beginning.len()].copy_from_slice(span_beginning.as_bytes());

                serial.write(buffer);
            }
            Message::Record(id) => {
                let mut buffer = [0; 256];

                let span_beginning = "Record ";
                buffer[0..span_beginning.len()].copy_from_slice(span_beginning.as_bytes());

                serial.write(buffer);
            }
            Message::Event => {
                let mut buffer = [0; 256];

                let span_beginning = "Event ";
                buffer[0..span_beginning.len()].copy_from_slice(span_beginning.as_bytes());

                serial.write(buffer);
            }
        };
    }
}
