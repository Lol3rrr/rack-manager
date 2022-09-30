use core::{
    future::Future, marker::PhantomData, sync::atomic, sync::atomic::AtomicBool, task::Waker,
};

use cortex_m::interrupt::InterruptNumber;
use general::AsyncSerial;
use hal::dma::CircReadDma;
use stm32l4xx_hal::{self as hal};

use super::NoInterruptMutex;

mod keys {
    use stm32l4xx_hal::{self as hal};

    macro_rules! key {
        ($name:ident, $interrupt_type:ty, $interrupt:expr) => {
            pub struct $name;

            impl crate::sealed::Sealed for $name {}

            impl NotifierKey for $name {
                type Interrupt = $interrupt_type;

                fn interrupt() -> Self::Interrupt {
                    $interrupt
                }
            }
        };
    }

    pub trait NotifierKey: crate::sealed::Sealed {
        type Interrupt: cortex_m::interrupt::InterruptNumber;

        fn interrupt() -> Self::Interrupt;
    }

    key!(
        Tx1Key,
        hal::stm32::Interrupt,
        hal::stm32::Interrupt::DMA1_CH4
    );
    key!(
        Rx1Key,
        hal::stm32::Interrupt,
        hal::stm32::Interrupt::DMA1_CH5
    );
    key!(
        Tx2Key,
        hal::stm32::Interrupt,
        hal::stm32::Interrupt::DMA1_CH7
    );
    key!(
        Rx2Key,
        hal::stm32::Interrupt,
        hal::stm32::Interrupt::DMA1_CH6
    );
}
pub use keys::*;

mod notifier {
    use super::*;

    /// This Notifier is needed to get the async part working.
    pub struct SerialNotifier<KEY> {
        waker: NoInterruptMutex<Option<Waker>>,
        complete: AtomicBool,
        _key: PhantomData<KEY>,
    }

    macro_rules! notifier {
        ($key:ty) => {
            impl SerialNotifier<$key> {
                pub const fn new() -> Self {
                    Self {
                        waker: NoInterruptMutex::new(None),
                        complete: AtomicBool::new(false),
                        _key: PhantomData {},
                    }
                }
            }
        };
    }

    notifier!(Tx2Key);
    notifier!(Rx2Key);

    impl<KEY> SerialNotifier<KEY>
    where
        KEY: NotifierKey,
    {
        pub(crate) fn set_waker(&self, waker: Waker) {
            self.waker.with_lock(|mut w| {
                *w = Some(waker);
            });
        }

        pub(crate) fn start_transfer(&self) {
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
}
pub use notifier::*;

pub trait Channel: crate::sealed::Sealed {
    fn listen(&mut self, event: hal::dma::Event);
}

pub trait DmaTx: crate::sealed::Sealed + Sized {
    type Key: NotifierKey + 'static;
    type Channel: Channel;

    fn to_dma(self, channel: Self::Channel) -> hal::dma::TxDma<Self, Self::Channel>;

    fn frame_tx(
        tx: hal::dma::TxDma<Self, Self::Channel>,
    ) -> hal::dma::FrameSender<
        &'static mut hal::dma::DMAFrame<256>,
        hal::dma::TxDma<Self, Self::Channel>,
        256,
    >;

    fn send_buffer(
        sender: &mut hal::dma::FrameSender<
            &'static mut hal::dma::DMAFrame<256>,
            hal::dma::TxDma<Self, Self::Channel>,
            256,
        >,
        buffer: &'static mut hal::dma::DMAFrame<256>,
    ) -> Result<(), &'static mut hal::dma::DMAFrame<256>>;

    fn transfer_complete(
        sender: &mut hal::dma::FrameSender<
            &'static mut hal::dma::DMAFrame<256>,
            hal::dma::TxDma<Self, Self::Channel>,
            256,
        >,
    ) -> Option<&'static mut hal::dma::DMAFrame<256>>;
}

pub trait DmaRx: crate::sealed::Sealed + Sized {
    type Key: NotifierKey + 'static;
    type Channel: Channel;

    fn to_dma(self, channel: Self::Channel) -> hal::dma::RxDma<Self, Self::Channel>;

    fn frame_rx(
        rx: hal::dma::RxDma<Self, Self::Channel>,
        buffer: &'static mut hal::dma::DMAFrame<256>,
    ) -> hal::dma::FrameReader<
        &'static mut hal::dma::DMAFrame<256>,
        hal::dma::RxDma<Self, Self::Channel>,
        256,
    >;
}

macro_rules! serial_tx {
    ($tx:ty, $tx_c:ty, $tx_k:ty) => {
        impl crate::sealed::Sealed for $tx_c {}
        impl Channel for $tx_c {
            fn listen(&mut self, event: hal::dma::Event) {
                <$tx_c>::listen(self, event);
            }
        }

        impl crate::sealed::Sealed for $tx {}
        impl DmaTx for $tx {
            type Key = $tx_k;
            type Channel = $tx_c;

            fn to_dma(self, channel: Self::Channel) -> hal::dma::TxDma<Self, Self::Channel> {
                self.with_dma(channel)
            }

            fn frame_tx(
                tx: hal::dma::TxDma<Self, Self::Channel>,
            ) -> hal::dma::FrameSender<
                &'static mut hal::dma::DMAFrame<256>,
                hal::dma::TxDma<Self, Self::Channel>,
                256,
            > {
                tx.frame_sender()
            }

            fn send_buffer(
                sender: &mut hal::dma::FrameSender<
                    &'static mut hal::dma::DMAFrame<256>,
                    hal::dma::TxDma<Self, Self::Channel>,
                    256,
                >,
                buffer: &'static mut hal::dma::DMAFrame<256>,
            ) -> Result<(), &'static mut hal::dma::DMAFrame<256>> {
                sender.send(buffer)
            }

            fn transfer_complete(
                sender: &mut hal::dma::FrameSender<
                    &'static mut hal::dma::DMAFrame<256>,
                    hal::dma::TxDma<Self, Self::Channel>,
                    256,
                >,
            ) -> Option<&'static mut hal::dma::DMAFrame<256>> {
                sender.transfer_complete_interrupt()
            }
        }
    };
}

macro_rules! serial_rx {
    ($rx:ty, $rx_c:ty, $rx_k:ty) => {
        impl crate::sealed::Sealed for $rx_c {}
        impl Channel for $rx_c {
            fn listen(&mut self, event: hal::dma::Event) {
                <$rx_c>::listen(self, event);
            }
        }

        impl crate::sealed::Sealed for $rx {}
        impl DmaRx for $rx {
            type Key = $rx_k;
            type Channel = $rx_c;

            fn to_dma(self, channel: Self::Channel) -> hal::dma::RxDma<Self, Self::Channel> {
                self.with_dma(channel)
            }

            fn frame_rx(
                rx: hal::dma::RxDma<Self, Self::Channel>,
                buffer: &'static mut hal::dma::DMAFrame<256>,
            ) -> hal::dma::FrameReader<
                &'static mut hal::dma::DMAFrame<256>,
                hal::dma::RxDma<Self, Self::Channel>,
                256,
            > {
                rx.frame_reader(buffer)
            }
        }
    };
}

serial_tx!(
    hal::serial::Tx<hal::stm32::USART1>,
    hal::dma::dma1::C4,
    Tx1Key
);
serial_rx!(
    hal::serial::Rx<hal::stm32::USART1>,
    hal::dma::dma1::C5,
    Rx1Key
);
serial_tx!(
    hal::serial::Tx<hal::stm32::USART2>,
    hal::dma::dma1::C7,
    Tx2Key
);
serial_rx!(
    hal::serial::Rx<hal::stm32::USART2>,
    hal::dma::dma1::C6,
    Rx2Key
);

struct SerialTx<TARGET>
where
    TARGET: DmaTx,
{
    tx: hal::dma::FrameSender<
        &'static mut hal::dma::DMAFrame<256>,
        hal::dma::TxDma<TARGET, TARGET::Channel>,
        256,
    >,
    tx_buffer: Option<&'static mut hal::dma::DMAFrame<256>>,
    notifier: &'static SerialNotifier<TARGET::Key>,
}

struct SerialRx<TARGET>
where
    TARGET: DmaRx,
{
    rx: hal::dma::FrameReader<
        &'static mut hal::dma::DMAFrame<256>,
        hal::dma::RxDma<TARGET, TARGET::Channel>,
        256,
    >,
    notifier: &'static SerialNotifier<TARGET::Key>,
}

impl<TARGET> SerialTx<TARGET>
where
    TARGET: DmaTx,
{
    pub fn new(
        mut tx: hal::dma::TxDma<TARGET, TARGET::Channel>,
        tx_buffer: &'static mut hal::dma::DMAFrame<256>,
        notifier: &'static SerialNotifier<TARGET::Key>,
    ) -> Self {
        tx.channel.listen(hal::dma::Event::TransferComplete);

        Self {
            tx: TARGET::frame_tx(tx),
            tx_buffer: Some(tx_buffer),
            notifier,
        }
    }

    pub fn write(&mut self, src: &[u8; 256]) -> TxFuture<'_, TARGET, hal::stm32::Interrupt> {
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

impl<TARGET> SerialRx<TARGET>
where
    TARGET: DmaRx,
{
    pub fn new(
        mut rx: hal::dma::RxDma<TARGET, TARGET::Channel>,
        rx_buffer_1: &'static mut hal::dma::DMAFrame<256>,
        notifier: &'static SerialNotifier<TARGET::Key>,
    ) -> Self {
        rx.channel.listen(hal::dma::Event::TransferComplete);

        Self {
            rx: <TARGET as DmaRx>::frame_rx(rx, rx_buffer_1),
            notifier,
        }
    }

    pub fn read(&mut self) -> RxFuture<'_, TARGET, hal::stm32::Interrupt> {
        RxFuture {
            rx: &mut self.rx,
            notifier: self.notifier,
            interrupt: Rx2Key::interrupt(),
        }
    }
}

pub struct TxFuture<'t, Tx, IT>
where
    Tx: DmaTx + 'static,
{
    notifier: &'static SerialNotifier<Tx::Key>,
    target_buffer: &'t mut Option<&'static mut hal::dma::DMAFrame<256>>,
    tx: &'t mut hal::dma::FrameSender<
        &'static mut hal::dma::DMAFrame<256>,
        hal::dma::TxDma<Tx, Tx::Channel>,
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

impl<'t, Tx, IT> Future for TxFuture<'t, Tx, IT>
where
    Tx: DmaTx,
    hal::dma::TxDma<Tx, Tx::Channel>: hal::dma::TransferPayload,
    IT: InterruptNumber + Unpin,
{
    type Output = ();

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        self.notifier.set_waker(cx.waker().clone());

        unsafe {
            cortex_m::peripheral::NVIC::unmask(self.interrupt);
        }

        match core::mem::replace(&mut self.state, TxState::Done) {
            TxState::Initial { data } => {
                self.notifier.start_transfer();
                match Tx::send_buffer(self.tx, data) {
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
            TxState::SendAndWaiting => match Tx::transfer_complete(self.tx) {
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

pub struct RxFuture<'t, Rx, IT>
where
    Rx: DmaRx + 'static,
{
    rx: &'t mut hal::dma::FrameReader<
        &'static mut hal::dma::DMAFrame<256>,
        hal::dma::RxDma<Rx, Rx::Channel>,
        256,
    >,
    notifier: &'static SerialNotifier<Rx::Key>,
    interrupt: IT,
}

impl<'t, Rx, IT> Future for RxFuture<'t, Rx, IT>
where
    Rx: DmaRx + 'static,
    IT: cortex_m::interrupt::InterruptNumber + Unpin,
{
    type Output = [u8; 256];

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        todo!()
    }
}

pub trait SerialKey: crate::sealed::Sealed {
    type Rx: DmaRx;
    type Tx: DmaTx;
}

macro_rules! usart {
    ($name:ident, $rx:ty, $tx:ty) => {
        pub struct $name {}

        impl crate::sealed::Sealed for $name {}
        impl SerialKey for $name {
            type Rx = $rx;
            type Tx = $tx;
        }
    };
}

usart!(
    USART1,
    hal::serial::Rx<hal::stm32::USART1>,
    hal::serial::Tx<hal::stm32::USART1>
);
usart!(
    USART2,
    hal::serial::Rx<hal::stm32::USART2>,
    hal::serial::Tx<hal::stm32::USART2>
);

pub struct Serial<SK>
where
    SK: SerialKey,
{
    rx: SerialRx<SK::Rx>,
    tx: SerialTx<SK::Tx>,
}

impl<SK> Serial<SK>
where
    SK: SerialKey,
{
    pub fn new(
        raw_tx: SK::Tx,
        raw_rx: SK::Rx,
        channel: (<SK::Tx as DmaTx>::Channel, <SK::Rx as DmaRx>::Channel),
        buffer: (
            &'static mut hal::dma::DMAFrame<256>,
            &'static mut hal::dma::DMAFrame<256>,
        ),
        notifier: (
            &'static SerialNotifier<<SK::Tx as DmaTx>::Key>,
            &'static SerialNotifier<<SK::Rx as DmaRx>::Key>,
        ),
    ) -> Self {
        let tx_dma = raw_tx.to_dma(channel.0);
        let rx_dma = raw_rx.to_dma(channel.1);

        let tx = SerialTx::new(tx_dma, buffer.0, notifier.0);
        let rx = SerialRx::new(rx_dma, buffer.1, notifier.1);

        Self { tx, rx }
    }
}

impl<SK> AsyncSerial<256> for Serial<SK>
where
    SK: 'static + SerialKey,
    hal::dma::TxDma<SK::Tx, <SK::Tx as DmaTx>::Channel>: hal::dma::TransferPayload,
{
    type ReceiveFuture<'t> = RxFuture<'t, SK::Rx, hal::stm32::Interrupt>;
    type WriteFuture<'t> = TxFuture<'t, SK::Tx, hal::stm32::Interrupt>;

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
