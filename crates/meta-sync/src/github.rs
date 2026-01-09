//! GitHub API interactions for fetching version manifests.

use crate::config::{GITHUB_OWNER, GITHUB_REPO, MANIFEST_PATH};
use crate::error::{Result, SyncError};
use octocrab::Octocrab;
use semver::Version;
use std::path::Path;

/// Represents a League of Legends game version
#[derive(Debug, Clone)]
pub struct GameVersion {
    /// Semantic version (e.g., "15.1.123456")
    pub version: String,
    /// URL to download the manifest file content
    pub download_url: String,
}

/// Fetches available League of Legends game versions from GitHub
pub async fn fetch_game_versions(octocrab: &Octocrab) -> Result<Vec<GameVersion>> {
    println!("🔍 Fetching game versions from {}/{}...", GITHUB_OWNER, GITHUB_REPO);
    
    let mut contents = octocrab
        .repos(GITHUB_OWNER, GITHUB_REPO)
        .get_content()
        .path(MANIFEST_PATH)
        .send()
        .await?;

    println!("📦 Found {} version files", contents.items.len());

    // Sort versions by semantic version
    contents.items.sort_by(|a, b| {
        let a_version = extract_version(&a.name).unwrap_or_default();
        let b_version = extract_version(&b.name).unwrap_or_default();
        a_version.cmp(&b_version)
    });

    // Convert to GameVersion and reverse (newest first)
    let versions: Vec<GameVersion> = contents
        .items
        .into_iter()
        .filter_map(|item| {
            let version = extract_version(&item.name)?;
            let download_url = item.download_url?.to_string();
            Some(GameVersion {
                version,
                download_url,
            })
        })
        .rev() // Process newest first
        .collect();

    Ok(versions)
}

/// Fetches the manifest URL from a GitHub file
pub async fn fetch_manifest_url(download_url: &str) -> Result<String> {
    let content = reqwest::get(download_url).await?;
    Ok(content.text().await?)
}

/// Extracts version string from a filename (e.g., "15.1.123456.txt" -> "15.1.123456")
fn extract_version(filename: &str) -> Option<String> {
    Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

/// Checks if a version should be processed based on the legacy cutoff
pub fn should_process_version(version: &str, cutoff: &str) -> Result<bool> {
    let cutoff_version = Version::parse(cutoff)
        .map_err(|_| SyncError::InvalidVersion(cutoff.to_string()))?;
    
    let current_version = Version::parse(version)
        .map_err(|_| SyncError::InvalidVersion(version.to_string()))?;

    if current_version <= cutoff_version {
        println!(
            "⏹️  Stopping at version {} - reached legacy cutoff (≤ {})",
            version, cutoff_version
        );
        return Ok(false);
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_version() {
        assert_eq!(
            extract_version("15.1.123456.txt"),
            Some("15.1.123456".to_string())
        );
        assert_eq!(
            extract_version("14.23.987654.txt"),
            Some("14.23.987654".to_string())
        );
    }

    #[test]
    fn test_should_process_version() {
        assert!(should_process_version("15.1.123456", "13.14.5227601").unwrap());
        assert!(!should_process_version("13.14.5227601", "13.14.5227601").unwrap());
        assert!(!should_process_version("13.13.0", "13.14.5227601").unwrap());
    }
}
