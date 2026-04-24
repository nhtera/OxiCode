//! Settings screen submodule — re-exports all four tab implementations.

pub mod general;
pub mod hooks;
pub mod permissions;
pub mod providers;

pub use general::GeneralTab;
pub use hooks::{HookEntry, HookGroup, HookSource, HooksTab};
pub use permissions::{DisplayPermMode, PermFocus, PermissionsTab};
pub use providers::{ProviderRow, ProviderStatus, ProvidersTab};
