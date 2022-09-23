#![no_std]
#![no_main]
#![feature(default_alloc_error_handler)]

use cortex_m::singleton;
use executor::tasks;
use hal::delay::Delay;
use hal::hal::delay::blocking::DelayUs;

extern crate cortex_m;
#[macro_use]
extern crate cortex_m_rt as rt;
extern crate cortex_m_semihosting as sh;
extern crate panic_semihosting;
extern crate stm32l4xx_hal as hal;

// use crate::hal::delay::Delay;
use crate::hal::prelude::*;
use crate::rt::entry;
use crate::rt::ExceptionFrame;
use hal::interrupt;

static SerialTxNotifier: utils::SerialNotifier<utils::Tx2Key> =
    utils::SerialNotifier::<utils::Tx2Key>::new();
static SerialRxNotifier: utils::SerialNotifier<utils::Rx2Key> =
    utils::SerialNotifier::<utils::Rx2Key>::new();

#[global_allocator]
static ALLOC: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

extern crate alloc;

#[entry]
fn main() -> ! {
    unsafe {
        ALLOC.lock().init((0x2000C000 + 8000) as *mut u8, 4000);
    }

    let cp = cortex_m::Peripherals::take().unwrap();
    let dp = hal::stm32::Peripherals::take().unwrap();

    dp.DMA1
        .ccr7
        .modify(|r, w| unsafe { w.bits(r.bits()) }.htie().bit(false));

    let mut flash = dp.FLASH.constrain();
    let mut rcc = dp.RCC.constrain();
    let mut pwr = dp.PWR.constrain(&mut rcc.apb1r1);

    // Try a different clock configuration

    let clocks = rcc.cfgr.freeze(&mut flash.acr, &mut pwr);

    let mut gpioa = dp.GPIOA.split(&mut rcc.ahb2);
    let channels = dp.DMA1.split(&mut rcc.ahb1);

    let mut gpiob = dp.GPIOB.split(&mut rcc.ahb2);
    let mut led = gpiob
        .pb3
        .into_push_pull_output(&mut gpiob.moder, &mut gpiob.otyper);

    let mut timer = Delay::new(cp.SYST, clocks);

    // The Serial API is highly generic
    // TRY the commented out, different pin configurations
    // let tx = gpioa.pa9.into_af7_pushpull(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrh);
    let tx = gpioa
        .pa2
        .into_alternate(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrl);
    // let tx = gpiob.pb6.into_alternate(&mut gpiob.moder, &mut gpiob.otyper, &mut gpiob.afrl);

    // let rx = gpioa.pa10.into_alternate(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrh);
    let rx = gpioa
        .pa3
        .into_alternate(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrl);
    // let rx = gpiob.pb7.into_alternate(&mut gpiob.moder, &mut gpiob.otyper, &mut gpiob.afrl);

    // TRY using a different USART peripheral here
    let serial_conf = hal::serial::Config::default()
        .baudrate(9600.bps())
        .parity_none()
        .stopbits(hal::serial::StopBits::STOP1P5)
        .oversampling(hal::serial::Oversampling::Over8);
    let serial =
        hal::serial::Serial::usart2(dp.USART2, (tx, rx), serial_conf, clocks, &mut rcc.apb1r1);

    let aserial = {
        let rx1 = singleton!(: [u8; 256] = [0; 256]).unwrap();
        let rx2 =
            singleton!(: stm32l4xx_hal::dma::DMAFrame<256> = stm32l4xx_hal::dma::DMAFrame::new())
                .unwrap();
        let tx =
            singleton!(: stm32l4xx_hal::dma::DMAFrame<256> = stm32l4xx_hal::dma::DMAFrame::new())
                .unwrap();
        utils::Serial::new(
            serial,
            (channels.7, channels.6),
            (tx, rx1, rx2),
            (&SerialTxNotifier, &SerialRxNotifier),
        )
    };
    led.set_high();

    let alloc = singleton!(: utils::allocator::LinkedListAllocator<1024> = utils::allocator::LinkedListAllocator::<1024>::new(
        0x2000C000 as *mut u8,
        (0x2000C000 + 8000) as *mut u8,
    )).unwrap();

    timer.delay_ms(500);

    led.set_low();

    timer.delay_ms(500);

    let (tx, rx) = utils::queue::unbounded::mpsc::queue(alloc);
    let (logger, logging_task) = utils::logging::logger(aserial, rx, tx);

    led.set_high();

    timer.delay_ms(500);

    tracing::subscriber::set_global_default(logger).unwrap();

    led.set_low();

    let span = tracing::span!(tracing::Level::INFO, "testing");
    let entered = span.enter();

    tracing::info!("testing2");

    drop(entered);

    /*
    let extension = Extension::init(
        gpioa
            .pa0
            .into_open_drain_output(&mut gpioa.moder, &mut gpioa.otyper),
        gpioa
            .pa1
            .into_pull_up_input(&mut gpioa.moder, &mut gpioa.pupdr),
        serial,
    )
    .expect("");

    let extension_task = extension.run(
        || [],
        |_| {},
        &[],
        |serial| {
            let rx1 =
            singleton!(: stm32l4xx_hal::dma::DMAFrame<256> = stm32l4xx_hal::dma::DMAFrame::new())
                .unwrap();
                let rx2 =
                singleton!(: stm32l4xx_hal::dma::DMAFrame<256> = stm32l4xx_hal::dma::DMAFrame::new())
                    .unwrap();
            let tx =
            singleton!(: stm32l4xx_hal::dma::DMAFrame<256> = stm32l4xx_hal::dma::DMAFrame::new())
                .unwrap();
            utils::Serial::new(serial, (channels.7, channels.6), (tx, rx1, rx2), (&SerialTxNotifier, &SerialRxNotifier))
        },
    );
    */

    tasks!(
        task_list,
        (send(), ext_task),
        (other(led, timer), test),
        (logging_task, lb)
    );

    let runtime = executor::Runtime::new(task_list);
    runtime.run();
}

async fn send() {
    loop {
        utils::futures::YieldNow::new().await;
    }
}

async fn other<PIN>(mut led: PIN, mut timer: Delay)
where
    PIN: embedded_hal::digital::blocking::OutputPin,
{
    loop {
        led.set_high();

        timer.delay_ms(500);

        led.set_low();

        timer.delay_ms(500);

        utils::futures::YieldNow::new().await;
    }
}

#[exception]
unsafe fn HardFault(ef: &ExceptionFrame) -> ! {
    panic!("{:#?}", ef);
}

#[interrupt]
fn DMA1_CH7() {
    SerialTxNotifier.transfer_complete();
}
#[interrupt]
fn DMA1_CH6() {
    SerialRxNotifier.transfer_complete();
}
