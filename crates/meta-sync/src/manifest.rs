//! RMAN manifest handling and binary downloading.

use crate::config::{CDN_URL, TARGET_BINARY};
use crate::error::{Result, SyncError};
use std::fs::File;
use std::io::Cursor;
use std::path::Path;

/// Downloads and parses an RMAN manifest, then extracts the target binary
pub async fn download_game_binary(
    manifest_url: &str,
    output_path: &Path,
) -> Result<()> {
    println!("  📥 Downloading manifest from CDN...");
    
    // Download manifest
    let manifest_response = reqwest::get(manifest_url)
        .await
        .map_err(|e| SyncError::Manifest(format!("Failed to download manifest: {}", e)))?;

    let manifest_bytes = manifest_response
        .bytes()
        .await
        .map_err(|e| SyncError::Manifest(format!("Failed to read manifest bytes: {}", e)))?;

    // Parse RMAN
    println!("  🔍 Parsing RMAN manifest...");
    let mut manifest_reader = Cursor::new(manifest_bytes);
    let manifest = rman::Manifest::read(&mut manifest_reader)
        .map_err(|e| SyncError::Manifest(format!("Failed to parse RMAN: {}", e)))?;

    println!("  📦 Manifest contains {} files", manifest.files.len());

    // Find target binary
    let target_file = manifest
        .files
        .iter()
        .find(|f| f.name == TARGET_BINARY)
        .ok_or_else(|| SyncError::BinaryNotFound {
            expected: TARGET_BINARY.to_string(),
        })?;

    println!("  ✓ Found target binary: {}", TARGET_BINARY);
    println!("  📥 Downloading binary chunks...");

    // Ensure output directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Download binary
    let mut output_file = File::create(output_path)?;
    
    target_file
        .download_all()
        .download(&mut ureq::Agent::new(), CDN_URL, &mut output_file)
        .map_err(|e| SyncError::Download(format!("Failed to download binary: {}", e)))?;

    println!("  ✓ Binary downloaded successfully");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_binary_constant() {
        assert_eq!(TARGET_BINARY, "LeagueofLegends.app/Contents/MacOS/LeagueofLegends");
    }
}
