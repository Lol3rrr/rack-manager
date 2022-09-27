use core::future::Future;

pub trait AsyncSerial<const N: usize> {
    type ReceiveFuture<'f>: Future<Output = [u8; N]>
    where
        Self: 'f;
    type WriteFuture<'f>: Future<Output = ()>
    where
        Self: 'f;

    fn read<'s, 'f>(&'s mut self) -> Self::ReceiveFuture<'f>
    where
        's: 'f;

    fn write<'s, 'f>(&'s mut self, buffer: [u8; N]) -> Self::WriteFuture<'f>
    where
        's: 'f;
}

#[cfg(False)]
pub mod mocks {
    use alloc::vec::Vec;
    use core::{future::Future, marker::PhantomData};

    use crate::AsyncSerial;

    pub struct MockSerial<const N: usize> {
        expected_reads: Vec<[u8; N]>,
        expected_writes: Vec<[u8; N]>,
    }

    impl<const N: usize> MockSerial<N> {
        pub fn new() -> Self {
            Self {
                expected_reads: Vec::new(),
                expected_writes: Vec::new(),
            }
        }

        pub fn read(&mut self, data: [u8; N]) {
            self.expected_reads.push(data);
        }
        pub fn write(&mut self, data: [u8; N]) {
            self.expected_writes.push(data);
        }

        pub fn assert_outstanding(&self) {
            assert!(self.expected_reads.is_empty());
            assert!(self.expected_writes.is_empty());
        }
    }

    impl<const N: usize> Default for MockSerial<N> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<const N: usize> AsyncSerial<N> for &mut MockSerial<N> {
        type ReceiveFuture<'f> = MockReceiveFuture<N> where Self: 'f;
        type WriteFuture<'f> = MockWriteFuture where Self: 'f;

        fn read<'s, 'f>(&'s mut self) -> Self::ReceiveFuture<'f>
        where
            's: 'f,
        {
            assert!(!self.expected_reads.is_empty(), "No more expected Reads");

            MockReceiveFuture {
                expected: self.expected_reads.remove(0),
            }
        }

        fn write<'s, 'f>(&'s mut self, buffer: [u8; N]) -> Self::WriteFuture<'f>
        where
            's: 'f,
        {
            assert!(!self.expected_writes.is_empty(), "No more expected Writes");

            let expected = self.expected_writes.remove(0);

            assert_eq!(expected, buffer);

            MockWriteFuture {}
        }
    }

    pub struct MockReceiveFuture<const N: usize> {
        expected: [u8; N],
    }
    impl<const N: usize> Future for MockReceiveFuture<N> {
        type Output = [u8; N];

        fn poll(
            self: core::pin::Pin<&mut Self>,
            _: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            core::task::Poll::Ready(self.expected)
        }
    }

    pub struct MockWriteFuture {}
    impl Future for MockWriteFuture {
        type Output = ();

        fn poll(
            self: core::pin::Pin<&mut Self>,
            _: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            core::task::Poll::Ready(())
        }
    }
}
