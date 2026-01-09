//! Dumper executable invocation and output handling.

use crate::config::Config;
use crate::error::{Result, SyncError};
use std::path::Path;
use std::process::Command;

/// Executes the dumper tool on a binary and saves the output
pub fn execute_dumper(
    config: &Config,
    binary_path: &Path,
    output_path: &Path,
) -> Result<()> {
    // Verify dumper exists
    if !config.dumper_path.exists() {
        return Err(SyncError::DumperNotFound {
            path: config.dumper_path.clone(),
        });
    }

    println!("  🔧 Executing dumper: {}", config.dumper_path.display());
    println!("     Input: {}", binary_path.display());
    println!("     Output: {}", output_path.display());

    // Ensure output directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let output = Command::new(&config.dumper_path)
        .arg(binary_path)
        .arg("--output")
        .arg(output_path)
        .output()
        .map_err(|e| SyncError::DumperFailed {
            message: format!("Failed to execute dumper: {}", e),
            stderr: String::new(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(SyncError::DumperFailed {
            message: format!("Dumper exited with code: {}", output.status),
            stderr,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        println!("  📋 Dumper output:\n{}", stdout);
    }

    println!("  ✓ Metaclasses dumped successfully");

    Ok(())
}

/// Cleans up temporary files after successful processing
pub fn cleanup_temp_file(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)
            .map_err(|e| SyncError::TempFile(format!("Failed to remove temp file: {}", e)))?;
        println!("  🧹 Cleaned up temporary file");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_cleanup_nonexistent_file() {
        let temp_path = Path::new("/tmp/nonexistent_file_12345.bin");
        assert!(cleanup_temp_file(temp_path).is_ok());
    }
}
