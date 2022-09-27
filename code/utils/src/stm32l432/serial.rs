use core::{
    future::Future, marker::PhantomData, sync::atomic, sync::atomic::AtomicBool, task::Waker,
};

use cortex_m::interrupt::InterruptNumber;
use general::AsyncSerial;
use hal::dma::CircReadDma;
use stm32l4xx_hal::{self as hal};

use super::NoInterruptMutex;

pub struct Tx2Key;
pub struct Rx2Key;

pub trait NotifierKey {
    type Interrupt: cortex_m::interrupt::InterruptNumber;

    fn interrupt() -> Self::Interrupt;
}

impl NotifierKey for Tx2Key {
    type Interrupt = hal::stm32::Interrupt;

    fn interrupt() -> Self::Interrupt {
        hal::stm32::Interrupt::DMA1_CH7
    }
}
impl NotifierKey for Rx2Key {
    type Interrupt = hal::stm32::Interrupt;

    fn interrupt() -> Self::Interrupt {
        hal::stm32::Interrupt::DMA1_CH6
    }
}

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

impl<KEY> SerialNotifier<KEY>
where
    KEY: NotifierKey,
{
    fn start_transfer(&self) {
        self.complete.store(false, atomic::Ordering::SeqCst);
    }

    pub fn transfer_complete(&self) {
        self.complete.store(true, atomic::Ordering::SeqCst);

        self.waker.with_lock(|waker| {
            if let Some(waker) = waker.as_ref() {
                waker.wake_by_ref();
            } else {
                // panic!("WHAT")
            }
        });

        cortex_m::peripheral::NVIC::mask(KEY::interrupt());
    }
}

pub struct SerialTx {
    tx: hal::dma::FrameSender<&'static mut hal::dma::DMAFrame<256>, hal::serial::TxDma2, 256>,
    tx_buffer: Option<&'static mut hal::dma::DMAFrame<256>>,
    notifier: &'static SerialNotifier<Tx2Key>,
}

pub struct SerialRx {
    rx: hal::dma::CircBuffer<[u8; 256], hal::serial::RxDma2>,
    notifier: &'static SerialNotifier<Rx2Key>,
}

impl SerialTx {
    pub fn new(
        mut tx: hal::serial::TxDma2,
        tx_buffer: &'static mut hal::dma::DMAFrame<256>,
        notifier: &'static SerialNotifier<Tx2Key>,
    ) -> Self {
        tx.channel.listen(hal::dma::Event::TransferComplete);

        Self {
            tx: tx.frame_sender(),
            tx_buffer: Some(tx_buffer),
            notifier,
        }
    }

    pub fn write(&mut self, src: &[u8; 256]) -> TxFuture<'_, hal::stm32::Interrupt> {
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
            interrupt: Tx2Key::interrupt(),
            state: TxState::Initial { data: buffer },
        }
    }
}

impl SerialRx {
    pub fn new(
        mut rx: hal::serial::RxDma2,
        rx_buffer_1: &'static mut [u8; 256],
        notifier: &'static SerialNotifier<Rx2Key>,
    ) -> Self {
        rx.channel.listen(hal::dma::Event::TransferComplete);

        Self {
            rx: rx.circ_read(rx_buffer_1),
            notifier,
        }
    }

    pub fn read(&mut self) -> RxFuture<'_, hal::stm32::Interrupt> {
        RxFuture {
            rx: &mut self.rx,
            notifier: self.notifier,
            pos: 0,
            buffer: [0; 256],
            interrupt: Rx2Key::interrupt(),
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

pub struct TxFuture<'t, IT> {
    notifier: &'static SerialNotifier<Tx2Key>,
    target_buffer: &'t mut Option<&'static mut hal::dma::DMAFrame<256>>,
    tx: &'t mut hal::dma::FrameSender<
        &'static mut hal::dma::DMAFrame<256>,
        hal::serial::TxDma2,
        256,
    >,
    interrupt: IT,
    state: TxState,
}
enum TxState {
    Initial {
        data: &'static mut hal::dma::DMAFrame<256>,
    },
    SendAndWaiting,
    Done,
}

impl<'t, IT> Future for TxFuture<'t, IT>
where
    IT: InterruptNumber + Unpin,
{
    type Output = ();

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        self.notifier.waker.with_lock(|mut w| {
            *w = Some(cx.waker().clone());
        });

        unsafe {
            cortex_m::peripheral::NVIC::unmask(self.interrupt);
        }

        match core::mem::replace(&mut self.state, TxState::Done) {
            TxState::Initial { data } => {
                self.notifier.start_transfer();
                match self.tx.send(data) {
                    Ok(_) => {
                        self.state = TxState::SendAndWaiting;

                        // cx.waker().wake_by_ref();

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

                    // assert!(self.notifier.complete.load(atomic::Ordering::SeqCst));

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

pub struct RxFuture<'t, IT> {
    rx: &'t mut hal::dma::CircBuffer<[u8; 256], hal::serial::RxDma2>,
    notifier: &'static SerialNotifier<Rx2Key>,
    pos: usize,
    buffer: [u8; 256],
    interrupt: IT,
}

impl<'t, IT> Future for RxFuture<'t, IT>
where
    IT: cortex_m::interrupt::InterruptNumber + Unpin,
{
    type Output = [u8; 256];

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        self.notifier.waker.with_lock(|mut w| {
            *w = Some(cx.waker().clone());
        });

        unsafe {
            cortex_m::peripheral::NVIC::unmask(self.interrupt);
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
        buffer: (&'static mut hal::dma::DMAFrame<256>, &'static mut [u8; 256]),
        notifier: (
            &'static SerialNotifier<Tx2Key>,
            &'static SerialNotifier<Rx2Key>,
        ),
    ) -> Self {
        let (raw_tx, raw_rx) = serial.split();

        let tx_dma = raw_tx.with_dma(channel.0);
        let rx_dma = raw_rx.with_dma(channel.1);

        let tx = SerialTx::new(tx_dma, buffer.0, notifier.0);
        let rx = SerialRx::new(rx_dma, buffer.1, notifier.1);

        Self { tx, rx }
    }

    pub fn split(self) -> (SerialTx, SerialRx) {
        (self.tx, self.rx)
    }
}

impl AsyncSerial<256> for Serial {
    type ReceiveFuture<'t> = RxFuture<'t, hal::stm32::Interrupt>;
    type WriteFuture<'t> = TxFuture<'t, hal::stm32::Interrupt>;

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
