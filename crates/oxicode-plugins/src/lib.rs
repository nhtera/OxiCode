pub mod install;
pub mod lifecycle;
pub mod manager;
pub mod manifest;
pub mod registry;
pub mod security;
pub mod subprocess;

pub use manager::PluginManager;
pub use manifest::{PluginManifest, PluginToolDef};
pub use registry::{PluginEntry, PluginRegistry};
pub use security::{TrustLevel, PermissionManifest};
