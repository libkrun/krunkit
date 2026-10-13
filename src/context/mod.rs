// SPDX-License-Identifier: Apache-2.0

#[cfg(not(windows))]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
pub use unix::KrunContext;
#[cfg(windows)]
pub use windows::KrunContext;
