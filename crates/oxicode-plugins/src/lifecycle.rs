//! Plugin lifecycle management: init and shutdown scripts.
//!
//! Plugins can declare optional init/shutdown scripts in their manifest.
//! These run as shell commands in the plugin's directory.

use std::path::Path;
use std::time::Duration;

use oxicode_common::{OxiError, OxiResult};
use tokio::process::Command;

/// Maximum time for a lifecycle script to run.
const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Run a lifecycle script (init or shutdown) in the plugin's directory.
pub async fn run_lifecycle_script(
    plugin_name: &str,
    script: &str,
    plugin_dir: &Path,
    phase: &str,
) -> OxiResult<()> {
    tracing::info!("Plugin '{plugin_name}': running {phase} script");

    let result = tokio::time::timeout(LIFECYCLE_TIMEOUT, async {
        let output = Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(plugin_dir)
            .output()
            .await
            .map_err(|e| {
                OxiError::Other(format!(
                    "Plugin '{plugin_name}' {phase} script failed to start: {e}"
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(OxiError::Other(format!(
                "Plugin '{plugin_name}' {phase} script exited with {}: {stderr}",
                output.status
            )));
        }

        Ok(())
    })
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(OxiError::Other(format!(
            "Plugin '{plugin_name}' {phase} script timed out after {}s",
            LIFECYCLE_TIMEOUT.as_secs()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_run_simple_script() {
        let dir = PathBuf::from("/tmp");
        let result = run_lifecycle_script("test", "echo hello", &dir, "init").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_failing_script() {
        let dir = PathBuf::from("/tmp");
        let result = run_lifecycle_script("test", "exit 1", &dir, "init").await;
        assert!(result.is_err());
    }
}
