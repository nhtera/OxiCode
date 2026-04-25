//! Library façade for `oxicode-cli`.
//!
//! Exposes internal modules so integration tests can import bridge types,
//! protocol definitions, and other testable components without duplicating code.
//!
//! The binary (`src/main.rs`) is the primary artifact; this lib is a thin
//! re-export shim for test harnesses only.

pub mod remote;

#[cfg(feature = "bridge")]
pub use remote::bridge;
#[cfg(feature = "bridge")]
pub use remote::jwt;
#[cfg(feature = "bridge")]
pub use remote::protocol;
