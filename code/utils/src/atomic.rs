#[cfg(not(loom))]
pub(crate) use core::sync::atomic::*;

#[cfg(loom)]
pub(crate) use loom::sync::atomic::*;
