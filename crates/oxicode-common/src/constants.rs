/// Application name.
pub const APP_NAME: &str = "oxicode";

/// Application display name.
pub const APP_DISPLAY_NAME: &str = "OxiCode";

/// Current version (from Cargo.toml).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default model to use when none specified.
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";

/// Default max tokens for response.
pub const DEFAULT_MAX_TOKENS: u32 = 16384;

/// Anthropic API base URL.
pub const ANTHROPIC_API_URL: &str = "https://api.anthropic.com";

/// Anthropic API version header value.
pub const ANTHROPIC_API_VERSION: &str = "2023-06-01";

/// Config directory name (inside user home).
pub const CONFIG_DIR_NAME: &str = ".oxicode";

/// Sessions subdirectory name.
pub const SESSIONS_DIR_NAME: &str = "sessions";

/// Config file name.
pub const CONFIG_FILE_NAME: &str = "settings.toml";

/// Claude MD file names to discover.
pub const CLAUDE_MD_FILES: &[&str] = &["OXICODE.md", "CLAUDE.md"];

/// Environment variable for API key.
pub const ENV_API_KEY: &str = "ANTHROPIC_API_KEY";

/// Environment variable for auth token (fallback when API key not set).
pub const ENV_AUTH_TOKEN: &str = "ANTHROPIC_AUTH_TOKEN";

/// Environment variable for custom Anthropic API base URL.
pub const ENV_BASE_URL: &str = "ANTHROPIC_BASE_URL";

/// Environment variable for default Haiku model name.
pub const ENV_DEFAULT_HAIKU_MODEL: &str = "ANTHROPIC_DEFAULT_HAIKU_MODEL";

/// Environment variable for default Sonnet model name.
pub const ENV_DEFAULT_SONNET_MODEL: &str = "ANTHROPIC_DEFAULT_SONNET_MODEL";

/// Environment variable for default Opus model name.
pub const ENV_DEFAULT_OPUS_MODEL: &str = "ANTHROPIC_DEFAULT_OPUS_MODEL";

/// Environment variable for model override.
pub const ENV_MODEL: &str = "OXICODE_MODEL";

/// Environment variable for config directory override.
pub const ENV_CONFIG_DIR: &str = "OXICODE_CONFIG_DIR";

/// Environment variable to override the auth header used for non-Anthropic proxies.
/// Accepted values: `bearer` | `x-api-key`. When unset, auto-detected from token prefix.
pub const ENV_AUTH_HEADER: &str = "OXICODE_AUTH_HEADER";
