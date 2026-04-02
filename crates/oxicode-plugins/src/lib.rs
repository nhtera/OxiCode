pub mod install;
pub mod lifecycle;
pub mod manager;
pub mod manifest;
pub mod security;
pub mod subprocess;

pub use manager::PluginManager;
pub use manifest::{PluginManifest, PluginToolDef};
