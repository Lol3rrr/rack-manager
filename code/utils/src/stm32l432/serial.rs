use core::{
    future::Future, marker::PhantomData, sync::atomic, sync::atomic::AtomicBool, task::Waker,
};

use embedded_hal::{nb::block, serial::nb::Read};
use general::AsyncSerial;
use hal::dma::CircReadDma;
use stm32l4xx_hal::{self as hal, device::tim1::dmar};

use super::NoInterruptMutex;

pub struct Tx2Key;
pub struct Rx2Key;

pub struct SerialNotifier<KEY> {
    waker: NoInterruptMutex<Option<Waker>>,
    complete: AtomicBool,
    _key: PhantomData<KEY>,
}

impl SerialNotifier<Tx2Key> {
    pub const fn new() -> Self {
        Self {
            waker: NoInterruptMutex::new(None),
            complete: AtomicBool::new(false),
            _key: PhantomData {},
        }
    }
}
impl SerialNotifier<Rx2Key> {
    pub const fn new() -> Self {
        Self {
            waker: NoInterruptMutex::new(None),
            complete: AtomicBool::new(false),
            _key: PhantomData {},
        }
    }
}

impl<KEY> SerialNotifier<KEY> {
    fn start_transfer(&self) {
        self.complete.store(false, atomic::Ordering::SeqCst);
    }

    pub fn transfer_complete(&self) {
        self.complete.store(true, atomic::Ordering::SeqCst);

        {
            let guard = self.waker.lock();
            if let Some(waker) = guard.as_ref() {
                waker.wake_by_ref();
            } else {
                panic!("WHAT")
            }
            drop(guard);
        }
    }
}

pub struct SerialTx {
    tx: hal::dma::FrameSender<&'static mut hal::dma::DMAFrame<256>, hal::serial::TxDma2, 256>,
    tx_buffer: Option<&'static mut hal::dma::DMAFrame<256>>,
    notifier: &'static SerialNotifier<Tx2Key>,
}

pub struct SerialRx {
    rx: hal::dma::CircBuffer<[u8; 256], hal::serial::RxDma2>,
    rx_buffer_1: Option<&'static mut hal::dma::DMAFrame<256>>,
    rx_buffer_2: Option<&'static mut hal::dma::DMAFrame<256>>,
    notifier: &'static SerialNotifier<Rx2Key>,
}

impl SerialTx {
    pub fn new(
        tx: hal::serial::TxDma2,
        tx_buffer: &'static mut hal::dma::DMAFrame<256>,
        notifier: &'static SerialNotifier<Tx2Key>,
    ) -> Self {
        Self {
            tx: tx.frame_sender(),
            tx_buffer: Some(tx_buffer),
            notifier,
        }
    }

    pub fn write(&mut self, src: &[u8; 256]) -> TxFuture<'_> {
        // Prepare the Buffer
        let buffer = self.tx_buffer.take().expect("");
        {
            let target = buffer.write();
            target.copy_from_slice(src);
            buffer.commit(256);
        }

        TxFuture {
            tx: &mut self.tx,
            target_buffer: &mut self.tx_buffer,
            notifier: self.notifier,
            state: TxState::Initial { data: buffer },
        }
    }
}

impl SerialRx {
    pub fn new(
        rx: hal::serial::RxDma2,
        rx_buffer_1: &'static mut [u8; 256],
        rx_buffer_2: &'static mut hal::dma::DMAFrame<256>,
        notifier: &'static SerialNotifier<Rx2Key>,
    ) -> Self {
        Self {
            rx: rx.circ_read(rx_buffer_1),
            rx_buffer_1: None,
            rx_buffer_2: Some(rx_buffer_2),
            notifier,
        }
    }

    pub fn read(&mut self) -> RxFuture<'_> {
        RxFuture {
            rx: &mut self.rx,
            notifier: self.notifier,
            pos: 0,
            buffer: [0; 256],
        }
    }

    pub fn split(
        self,
    ) -> (
        hal::serial::Rx<hal::pac::USART2>,
        hal::dma::dma1::C6,
        &'static mut [u8; 256],
    ) {
        let (buffer, rx_dma) = self.rx.stop();
        let (rx, channel) = rx_dma.split();
        (rx, channel, buffer)
    }
}

pub struct TxFuture<'t> {
    notifier: &'static SerialNotifier<Tx2Key>,
    target_buffer: &'t mut Option<&'static mut hal::dma::DMAFrame<256>>,
    tx: &'t mut hal::dma::FrameSender<
        &'static mut hal::dma::DMAFrame<256>,
        hal::serial::TxDma2,
        256,
    >,
    state: TxState,
}
enum TxState {
    Initial {
        data: &'static mut hal::dma::DMAFrame<256>,
    },
    SendAndWaiting,
    Done,
}

impl<'t> Future for TxFuture<'t> {
    type Output = ();

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        {
            let mut guard = self.notifier.waker.lock();
            *guard = Some(cx.waker().clone());
            drop(guard);
        }

        match core::mem::replace(&mut self.state, TxState::Done) {
            TxState::Initial { data } => {
                self.notifier.start_transfer();
                match self.tx.send(data) {
                    Ok(_) => {
                        self.state = TxState::SendAndWaiting;

                        cx.waker().wake_by_ref();

                        core::task::Poll::Pending
                    }
                    Err(data) => {
                        self.state = TxState::Initial { data };

                        cx.waker().wake_by_ref();

                        core::task::Poll::Pending
                    }
                }
            }
            TxState::SendAndWaiting => match self.tx.transfer_complete_interrupt() {
                Some(buffer) => {
                    *self.target_buffer = Some(buffer);

                    self.state = TxState::Done;

                    core::task::Poll::Ready(())
                }
                None => {
                    self.state = TxState::SendAndWaiting;

                    cx.waker().wake_by_ref();

                    core::task::Poll::Pending
                }
            },
            TxState::Done => {
                self.state = TxState::Done;
                core::task::Poll::Ready(())
            }
        }
    }
}

pub struct RxFuture<'t> {
    rx: &'t mut hal::dma::CircBuffer<[u8; 256], hal::serial::RxDma2>,
    notifier: &'static SerialNotifier<Rx2Key>,
    pos: usize,
    buffer: [u8; 256],
}

impl<'t> Future for RxFuture<'t> {
    type Output = [u8; 256];

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        {
            let mut guard = self.notifier.waker.lock();
            *guard = Some(cx.waker().clone());
            drop(guard);
        }

        cx.waker().wake_by_ref();

        let c_pos = self.pos;
        let remaining = &mut self.buffer[c_pos..];
        if remaining.is_empty() {
            core::task::Poll::Ready(self.buffer)
        } else {
            let mut tmp = [0; 256];

            match self.rx.read(&mut tmp[c_pos..]) {
                Ok(read_size) => {
                    self.pos += read_size;
                    self.buffer[c_pos..].copy_from_slice(&tmp[c_pos..]);

                    cx.waker().wake_by_ref();

                    core::task::Poll::Pending
                }
                Err(_) => {
                    let error_string = b"Error";
                    self.buffer = [0; 256];
                    self.pos = 255;

                    self.buffer[0..error_string.len()].copy_from_slice(error_string);

                    core::task::Poll::Ready(self.buffer)
                }
            }
        }
    }
}

pub struct Serial {
    rx: SerialRx,
    tx: SerialTx,
}

impl Serial {
    pub fn new<PINS>(
        serial: hal::serial::Serial<hal::pac::USART2, PINS>,
        channel: (hal::dma::dma1::C7, hal::dma::dma1::C6),
        buffer: (
            &'static mut hal::dma::DMAFrame<256>,
            &'static mut [u8; 256],
            &'static mut hal::dma::DMAFrame<256>,
        ),
        notifier: (
            &'static SerialNotifier<Tx2Key>,
            &'static SerialNotifier<Rx2Key>,
        ),
    ) -> Self {
        let (raw_tx, raw_rx) = serial.split();

        let tx_dma = raw_tx.with_dma(channel.0);
        let rx_dma = raw_rx.with_dma(channel.1);

        let tx = SerialTx::new(tx_dma, buffer.0, notifier.0);
        let rx = SerialRx::new(rx_dma, buffer.1, buffer.2, notifier.1);

        Self { tx, rx }
    }

    pub fn split(self) -> (SerialTx, SerialRx) {
        (self.tx, self.rx)
    }
}

impl AsyncSerial<256> for Serial {
    type ReceiveFuture<'t> = RxFuture<'t>;
    type WriteFuture<'t> = TxFuture<'t>;

    fn read<'s, 'f>(&'s mut self) -> Self::ReceiveFuture<'f>
    where
        's: 'f,
    {
        self.rx.read()
    }

    fn write<'s, 'f>(&'s mut self, buffer: [u8; 256]) -> Self::WriteFuture<'f>
    where
        's: 'f,
    {
        self.tx.write(&buffer)
    }
}
