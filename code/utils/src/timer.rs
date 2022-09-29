//! # Design
//! The Timer itself is built on a timer-wheel, however we limit the core timer-wheel to a
//! relatively small size for efficiency and to save on memory. We will most likely have 2 "Layers"
//! of timer-wheels.
//! However to also cover very long running timers, we add an extra priority-queue which stores all
//! timers that would exceed the limit of the Timer wheel. Then on every tick of the wheel, we
//! check the priority queue to see if any from it can now be moved into the timer wheel.
//!
//! ## The Timer wheels
//! The Timer wheels will store all the Timers that should fire in the near future.
//! The first level wheel, has a slot for every timestep we support, making it the most fine grained.
//! The second level wheel, will have a slot for every first level wheel. Once the first wheel
//! reaches the end and starts from the beginning again, we will also move the Timers from the second
//! wheel down into the first wheel and advance it as well.
//!
//! ## The Priority Queue
//! As the priority, we use the time-delta until the timer needs to fire. Every time we insert a
//! new entry into the Queue, we recalculate all the priorities before inserting the new Element.
//! This allows us to mostly leave the queue alone and only update it, when needed.

pub mod fixed_size {
    //! This relies on a series of hierarchical timer wheels and only has the space for a fixed
    //! number of Timers running at the same time. This makes it less flexible, but also avoids
    //! any dynamic memory allocations.
    //!
    //! # Collisions
    //! To keep its goal of no dynamic memory allocations, we will only use fixed size arrays.
    //! This however results in potential collisions between timers, as they may belong into the
    //! same slot in the wheel. This is solved, by losing some accuracy in these cases, by
    //! performing a linear search for a free slot in the rest of the timer wheel or higher ones
    //! in the hierarchie.

    use core::{future::Future, marker::PhantomData, num::NonZeroUsize, task::Waker};

    use crate::{
        atomic::{self, AtomicBool, AtomicIsize, AtomicU8, AtomicUsize},
        unsafecell::UnsafeCell,
    };

    pub trait Timescale {
        fn scale_ms(time: usize) -> usize;
    }

    pub struct ScaleGeneral<const N: usize> {}

    impl<const N: usize> Timescale for ScaleGeneral<N> {
        fn scale_ms(time: usize) -> usize {
            if time % N == 0 {
                time / N
            } else {
                (time / N) + 1
            }
        }
    }

    pub type Scale1Ms = ScaleGeneral<1>;
    pub type Scale10Ms = ScaleGeneral<10>;
    pub type Scale100Ms = ScaleGeneral<100>;

    pub struct Slot {
        state: AtomicU8,
        waker: UnsafeCell<Option<Waker>>,
        fired: AtomicBool,
    }

    impl Slot {
        const fn new() -> Self {
            Self {
                state: AtomicU8::new(0),
                waker: UnsafeCell::new(None),
                fired: AtomicBool::new(false),
            }
        }
    }

    pub struct SlotStorage<const N: usize> {
        wakers: [Slot; N],
        used_slots: AtomicUsize,
    }

    impl<const N: usize> SlotStorage<N> {
        const fn new() -> Self {
            #[allow(clippy::declare_interior_mutable_const)]
            const SLOT: Slot = Slot::new();

            Self {
                wakers: [SLOT; N],
                used_slots: AtomicUsize::new(0),
            }
        }

        fn add_waker(&self, waker: Waker) -> Result<usize, ()> {
            let usage = self.used_slots.fetch_add(1, atomic::Ordering::SeqCst);
            if usage >= N {
                self.used_slots.fetch_sub(1, atomic::Ordering::SeqCst);
                return Err(());
            }

            loop {
                for (index, slot) in self.wakers.iter().enumerate() {
                    if slot.state.load(atomic::Ordering::Relaxed) != 0 {
                        continue;
                    }

                    if slot
                        .state
                        .compare_exchange(0, 1, atomic::Ordering::SeqCst, atomic::Ordering::SeqCst)
                        .is_err()
                    {
                        continue;
                    }

                    slot.fired.store(false, atomic::Ordering::SeqCst);

                    slot.waker.with_mut(|w| {
                        let w_ref = unsafe { &mut *w };
                        *w_ref = Some(waker);
                    });

                    slot.state.store(2, atomic::Ordering::SeqCst);

                    return Ok(index);
                }
            }
        }

        fn take_slot(&self, index: usize) -> Option<(Waker, &AtomicBool)> {
            let slot = self.wakers.get(index)?;

            if slot
                .state
                .compare_exchange(2, 1, atomic::Ordering::SeqCst, atomic::Ordering::SeqCst)
                .is_err()
            {
                return None;
            }

            let data = slot.waker.with_mut(|raw_w| {
                let w_ref = unsafe { &mut *raw_w };
                w_ref.take()
            })?;

            let fired_ref = &slot.fired;

            Some((data, fired_ref))
        }
    }
    impl<const N: usize> AsRef<[Slot]> for SlotStorage<N> {
        fn as_ref(&self) -> &[Slot] {
            &self.wakers
        }
    }

    pub struct LevelOneWheel {
        current: AtomicUsize,
        slots: [AtomicIsize; 32],
    }

    pub struct LevelTwoWheel {
        slots: [LevelOneWheel; 32],
    }

    pub struct LevelThreeWheel {
        slots: [LevelTwoWheel; 32],
    }

    pub struct TimerWheel<WHEEL, SCALE>
    where
        WHEEL: Wheel,
        SCALE: Timescale,
    {
        wheel: WHEEL,
        waker: WHEEL::Storage,
        _marker: PhantomData<SCALE>,
    }

    unsafe impl<WHEEL, SCALE> core::marker::Sync for TimerWheel<WHEEL, SCALE>
    where
        WHEEL: Wheel,
        SCALE: Timescale,
    {
    }

    impl LevelOneWheel {
        const fn new() -> Self {
            #[allow(clippy::declare_interior_mutable_const)]
            const SLOT: AtomicIsize = AtomicIsize::new(-1);

            Self {
                current: AtomicUsize::new(0),
                slots: [SLOT; 32],
            }
        }
    }
    impl LevelTwoWheel {
        const fn new() -> Self {
            #[allow(clippy::declare_interior_mutable_const)]
            const WHEEL: LevelOneWheel = LevelOneWheel::new();

            Self { slots: [WHEEL; 32] }
        }
    }
    impl LevelThreeWheel {
        const fn new() -> Self {
            #[allow(clippy::declare_interior_mutable_const)]
            const WHEEL: LevelTwoWheel = LevelTwoWheel::new();

            Self { slots: [WHEEL; 32] }
        }
    }

    impl<SCALE> TimerWheel<LevelOneWheel, SCALE>
    where
        SCALE: Timescale,
    {
        pub const fn new() -> Self {
            Self {
                wheel: LevelOneWheel::new(),
                waker: SlotStorage::new(),
                _marker: PhantomData {},
            }
        }
    }
    impl<SCALE> TimerWheel<LevelTwoWheel, SCALE>
    where
        SCALE: Timescale,
    {
        pub const fn new() -> Self {
            Self {
                wheel: LevelTwoWheel::new(),
                waker: SlotStorage::new(),
                _marker: PhantomData {},
            }
        }
    }

    pub enum TimerHandle<'t> {
        Registered {
            slot: &'t Slot,
            used_slots: &'t AtomicUsize,
        },
        Fired,
    }

    impl<'t> Drop for TimerHandle<'t> {
        fn drop(&mut self) {
            match self {
                Self::Registered { slot, used_slots } => {
                    slot.state.store(0, atomic::Ordering::SeqCst);
                    slot.fired.store(false, atomic::Ordering::SeqCst);

                    used_slots.fetch_sub(1, atomic::Ordering::SeqCst);
                }
                Self::Fired => {}
            };
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    pub enum WheelAddError {
        OutOfRange,
        Full,
        Other(&'static str),
    }

    pub trait Wheel {
        type Storage: AsRef<[Slot]>;

        fn tick(&self, storage: &Self::Storage);

        fn add_step<'t>(
            &self,
            time: NonZeroUsize,
            waker: Waker,
            storage: &'t Self::Storage,
        ) -> Result<TimerHandle<'t>, WheelAddError>;
    }

    impl Wheel for LevelOneWheel {
        type Storage = SlotStorage<32>;

        fn tick(&self, storage: &Self::Storage) {
            let index = (self.current.fetch_add(1, atomic::Ordering::SeqCst) + 1) % 32;

            let slot = &self.slots[index];

            let waker_index = match slot.load(atomic::Ordering::SeqCst) {
                id if id < 0 => return,
                id => id as usize,
            };

            if slot
                .compare_exchange(
                    waker_index as isize,
                    -1,
                    atomic::Ordering::SeqCst,
                    atomic::Ordering::SeqCst,
                )
                .is_err()
            {
                return;
            }

            let (waker, fired) = storage.take_slot(waker_index).unwrap();
            fired.store(true, atomic::Ordering::SeqCst);

            waker.wake();
        }
        fn add_step<'t>(
            &self,
            time: NonZeroUsize,
            waker: Waker,
            storage: &'t Self::Storage,
        ) -> Result<TimerHandle<'t>, WheelAddError> {
            if time.get() >= 32 {
                return Err(WheelAddError::OutOfRange);
            }

            let waker_index = storage.add_waker(waker).map_err(|e| WheelAddError::Full)? as isize;

            for i in 0..31 {
                let slot_index =
                    (self.current.load(atomic::Ordering::SeqCst) + time.get() + i) % 32;

                let slot = &self.slots[slot_index];

                if slot
                    .compare_exchange(
                        -1,
                        waker_index,
                        atomic::Ordering::SeqCst,
                        atomic::Ordering::SeqCst,
                    )
                    .is_ok()
                {
                    return Ok(TimerHandle::Registered {
                        slot: &storage.wakers[waker_index as usize],
                        used_slots: &storage.used_slots,
                    });
                }
            }

            Err(WheelAddError::Full)
        }
    }
    impl Wheel for LevelTwoWheel {
        // 32 * 32 = 1024
        type Storage = SlotStorage<1024>;

        fn tick(&self, storage: &Self::Storage) {
            todo!("Tick Level Two Wheel")
        }
        fn add_step<'t>(
            &self,
            time: NonZeroUsize,
            waker: Waker,
            storage: &'t Self::Storage,
        ) -> Result<TimerHandle<'t>, WheelAddError> {
            todo!("Add")
        }
    }

    impl<WHEEL, SCALE> TimerWheel<WHEEL, SCALE>
    where
        WHEEL: Wheel,
        SCALE: Timescale,
    {
        pub fn tick(&self) {
            self.wheel.tick(&self.waker);
        }

        /// Adds the Waker to be woken in the given time in ms.
        ///
        /// # Special Case
        /// If `time` is 0, the waker is immediately woken and not woken after that again
        fn add_ms(&self, time: usize, waker: Waker) -> Result<TimerHandle, WheelAddError> {
            match NonZeroUsize::new(time) {
                Some(time) => self.wheel.add_step(time, waker, &self.waker),
                None => {
                    waker.wake();
                    Ok(TimerHandle::Fired)
                }
            }
        }

        #[cfg(feature = "stm32l432")]
        pub fn configure_tim3(
            &self,
            timer: stm32l4xx_hal::pac::TIM3,
            clocks: stm32l4xx_hal::rcc::Clocks,
            timeout: impl Into<stm32l4xx_hal::time::Hertz>,
            bus: &mut stm32l4xx_hal::rcc::APB1R1,
        ) {
            use stm32l4xx_hal::rcc::{Enable, Reset};

            stm32l4xx_hal::pac::TIM6::enable(bus);
            stm32l4xx_hal::pac::TIM6::reset(bus);

            // Pause Timer
            timer.cr1.modify(|_, w| w.cen().clear_bit());

            let clock = clocks.timclk1();
            let timeout = timeout.into();

            // Find Prescaler
            let psc = 15;
            let arr = 4999;

            timer.psc.write(|w| w.psc().bits(psc));
            timer.arr.write(|w| unsafe { w.bits(arr) });

            timer.egr.write(|w| w.ug().set_bit());

            // Trigger an update event to load the prescaler value to the clock.
            timer.egr.write(|w| w.ug().set_bit());

            timer.sr.modify(|_, w| w.uif().clear_bit());

            unsafe {
                cortex_m::peripheral::NVIC::unmask(stm32l4xx_hal::stm32::Interrupt::TIM7);
            }

            timer.cr1.write(|w| {
                w.cen()
                    .set_bit()
                    .udis()
                    .clear_bit()
                    .arpe()
                    .set_bit()
                    .urs()
                    .any_event()
            });
            timer.cnt.write(|w| w.cnt().bits(0));

            //timer.dier.write(|w| w.uie().set_bit());
        }

        #[cfg(feature = "stm32l432")]
        pub fn clear_interrupt_tim3(&self) {
            let raw_regs = stm32l4xx_hal::pac::TIM3::ptr();
            let regs = unsafe { &*raw_regs };

            regs.sr.write(|w| w.uif().clear_bit());
        }
    }

    impl<SCALE> TimerWheel<LevelOneWheel, SCALE>
    where
        SCALE: Timescale,
    {
        pub fn sleep_ms(&self, time: usize) -> SleepMs<'_, LevelOneWheel, SCALE> {
            SleepMs {
                timer: self,
                handle: None,
                time: SCALE::scale_ms(time),
            }
        }
    }

    pub struct SleepMs<'t, WHEEL, SCALE>
    where
        WHEEL: Wheel,
        SCALE: Timescale,
    {
        timer: &'t TimerWheel<WHEEL, SCALE>,
        handle: Option<TimerHandle<'t>>,
        time: usize,
    }

    impl<'t, WHEEL, SCALE> Future for SleepMs<'t, WHEEL, SCALE>
    where
        WHEEL: Wheel,
        SCALE: Timescale,
    {
        type Output = Result<(), ()>;

        fn poll(
            mut self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            match &self.handle {
                Some(handle) => match handle {
                    TimerHandle::Fired => core::task::Poll::Ready(Ok(())),
                    TimerHandle::Registered { slot, used_slots } => {
                        if slot.fired.load(atomic::Ordering::SeqCst) {
                            core::task::Poll::Ready(Ok(()))
                        } else {
                            core::task::Poll::Pending
                        }
                    }
                },
                None => {
                    let handle = match self.timer.add_ms(self.time, cx.waker().clone()) {
                        Ok(h) => h,
                        Err(e) => return core::task::Poll::Ready(Err(())),
                    };
                    self.handle = Some(handle);

                    core::task::Poll::Pending
                }
            }
        }
    }

    #[cfg(all(test, not(loom)))]
    mod tests {
        use super::*;

        #[test]
        fn scale_1ms() {
            assert_eq!(1, Scale1Ms::scale_ms(1));
            assert_eq!(2, Scale1Ms::scale_ms(2));
            assert_eq!(3, Scale1Ms::scale_ms(3));
        }

        #[test]
        fn scale_10ms() {
            assert_eq!(0, Scale10Ms::scale_ms(0));
            assert_eq!(1, Scale10Ms::scale_ms(1));
            assert_eq!(1, Scale10Ms::scale_ms(9));
            assert_eq!(1, Scale10Ms::scale_ms(10));
            assert_eq!(2, Scale10Ms::scale_ms(11));
        }

        #[test]
        fn storage_add_waker() {
            let storage = SlotStorage::<2>::new();

            assert_eq!(Ok(0), storage.add_waker(futures_test::task::noop_waker()));
        }

        #[test]
        fn storage_add_wakers_full() {
            let storage = SlotStorage::<2>::new();

            assert_eq!(Ok(0), storage.add_waker(futures_test::task::noop_waker()));
            assert_eq!(Ok(1), storage.add_waker(futures_test::task::noop_waker()));
            assert_eq!(Err(()), storage.add_waker(futures_test::task::noop_waker()));
        }

        #[test]
        fn storage_take_waker() {
            let storage = SlotStorage::<2>::new();

            assert_eq!(Ok(0), storage.add_waker(futures_test::task::noop_waker()));

            assert!(storage.take_slot(0).is_some());

            assert_eq!(Ok(1), storage.add_waker(futures_test::task::noop_waker()));
        }

        #[test]
        fn add_0ms_timer1() {
            let timer = TimerWheel::<LevelOneWheel, Scale1Ms>::new();

            let (waker, count) = futures_test::task::new_count_waker();

            let handle_res = timer.add_ms(0, waker).unwrap();
            assert!(matches!(handle_res, TimerHandle::Fired));

            assert_eq!(1, count.get());
        }

        #[test]
        fn add_1ms_timer1() {
            let timer = TimerWheel::<LevelOneWheel, Scale1Ms>::new();

            let (waker, count) = futures_test::task::new_count_waker();

            let handle_res = timer.add_ms(1, waker).unwrap();
            assert!(matches!(handle_res, TimerHandle::Registered { .. }));
            assert_eq!(0, count.get());
        }

        #[test]
        fn timer1_add_tick() {
            let timer = TimerWheel::<LevelOneWheel, Scale1Ms>::new();

            let (waker, count) = futures_test::task::new_count_waker();

            let handle_res = timer.add_ms(1, waker).unwrap();
            assert!(matches!(handle_res, TimerHandle::Registered { .. }));
            assert_eq!(0, count.get());

            timer.tick();

            assert_eq!(1, count.get());
        }

        #[test]
        fn sleep_future_1ms() {
            let timer = TimerWheel::<LevelOneWheel, Scale1Ms>::new();

            let mut sleep_fut = Box::pin(timer.sleep_ms(2));

            let (waker, count) = futures_test::task::new_count_waker();
            let mut ctx = core::task::Context::from_waker(&waker);

            let res = sleep_fut.as_mut().poll(&mut ctx);
            assert!(res.is_pending());
            assert_eq!(0, count.get());

            timer.tick();

            let res = sleep_fut.as_mut().poll(&mut ctx);
            assert!(res.is_pending());
            assert_eq!(0, count.get());

            timer.tick();

            let res = sleep_fut.as_mut().poll(&mut ctx);
            assert!(res.is_ready());
            assert_eq!(1, count.get());
        }

        #[test]
        fn sleep_future_10ms() {
            let timer = TimerWheel::<LevelOneWheel, Scale10Ms>::new();

            let mut sleep_fut = Box::pin(timer.sleep_ms(250));

            let (waker, count) = futures_test::task::new_count_waker();
            let mut ctx = core::task::Context::from_waker(&waker);

            for _ in 0..24 {
                let res = sleep_fut.as_mut().poll(&mut ctx);
                assert!(res.is_pending());
                assert_eq!(0, count.get());

                timer.tick();
            }

            let res = sleep_fut.as_mut().poll(&mut ctx);
            assert!(res.is_pending());
            assert_eq!(0, count.get());

            timer.tick();

            let res = sleep_fut.as_mut().poll(&mut ctx);
            assert!(res.is_ready());
            assert_eq!(1, count.get());
        }
    }
}
