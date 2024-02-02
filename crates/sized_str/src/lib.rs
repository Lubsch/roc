// This should be no_std, but we want to be able to use dbg! in development and std conveniences in testing
// Having this be no_std isn't strictly necessary, but it reduces the risk of accidental heap allocations.
#![cfg_attr(not(any(debug_assertions, test)), no_std)]

mod sized_str;
mod str_finder;

pub use crate::sized_str::*;
pub use crate::str_finder::*;