#![cfg_attr(not(test), no_std)]

#[cfg(feature = "stm32l432")]
mod stm32l432;
#[cfg(feature = "stm32l432")]
pub use stm32l432::*;
