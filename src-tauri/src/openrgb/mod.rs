//! Minimal OpenRGB SDK client (protocol version 3), used to mirror the case
//! lighting colour onto a motherboard header.

pub mod client;
#[cfg(test)]
mod fake;
pub mod protocol;
pub mod sync;
