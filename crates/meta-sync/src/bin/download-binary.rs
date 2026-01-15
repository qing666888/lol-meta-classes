//! Download a League of Legends binary for a specific version.
//!
//! This tool downloads the macOS League of Legends binary from Riot's CDN
//! so you can analyze it in IDA, Ghidra, or other disassemblers.
//!
//! ## Usage
//!
//! ```bash
//! # Download a specific version
//! cargo run --release --bin download-binary -- 16.1.7374870
//!
//! # Download to a custom output path
//! cargo run --release --bin download-binary -- 16.1.7374870 -o /tmp/lol_16.1.bin
//!
//! # List available versions
//! cargo run --release --bin download-binary -- --list
//! ```

use clap::Parser;
use std::io::Cursor;
use std::path::PathBuf;

/// CDN base URL for downloading League of Legends files
const CDN_URL: &str = "http://lol.secure.dyn.riotcdn.net/channels/public/bundles";

/// GitHub repository info
const GITHUB_OWNER: &str = "Morilli";
const GITHUB_REPO: &str = "riot-manifests";
const MANIFEST_PATH: &str = "LoL/EUW1/macos/lol-game-client";

/// The specific binary file we're looking for in the manifest
const TARGET_BINARY: &str = "LeagueofLegends.app/Contents/MacOS/LeagueofLegends";

#[derive(Parser)]
#[command(name = "download-binary")]
#[command(about = "Download League of Legends binary for analysis in IDA/Ghidra")]
struct Args {
    /// Version to download (e.g., "16.1.7374870")
    #[arg(value_name = "VERSION")]
    version: Option<String>,

    /// Output file path (defaults to ./{version}.bin)
    #[arg(short, long, value_name = "PATH")]
    output: Option<PathBuf>,

    /// List available versions
    #[arg(short, long)]
    list: bool,

    /// Show the N most recent versions when listing
    #[arg(short = 'n', long, default_value = "20")]
    count: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.list {
        list_versions(args.count).await?;
        return Ok(());
    }

    let version = args.version.ok_or("Please provide a version to download. Use --list to see available versions.")?;

    let output = args.output.unwrap_or_else(|| PathBuf::from(format!("{}.bin", version)));

    download_binary(&version, &output).await?;

    Ok(())
}

async fn list_versions(count: usize) -> Result<(), Box<dyn std::error::Error>> {
    println!("Fetching available versions from {}/{}...", GITHUB_OWNER, GITHUB_REPO);

    let octocrab = octocrab::Octocrab::default();

    let contents = octocrab
        .repos(GITHUB_OWNER, GITHUB_REPO)
        .get_content()
        .path(MANIFEST_PATH)
        .send()
        .await?;

    // Extract and sort versions
    let mut versions: Vec<String> = contents
        .items
        .iter()
        .filter_map(|item| {
            std::path::Path::new(&item.name)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .collect();

    // Sort by semver (newest first)
    versions.sort_by(|a, b| {
        let a_ver = semver::Version::parse(a).ok();
        let b_ver = semver::Version::parse(b).ok();
        b_ver.cmp(&a_ver)
    });

    println!("\nAvailable versions (showing {} most recent):\n", count.min(versions.len()));

    for version in versions.iter().take(count) {
        println!("  {}", version);
    }

    println!("\nTotal: {} versions available", versions.len());
    println!("\nUsage: download-binary <VERSION> [-o output.bin]");

    Ok(())
}

async fn download_binary(version: &str, output: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("Downloading League of Legends binary");
    println!("  Version: {}", version);
    println!("  Output:  {}", output.display());
    println!();

    // Step 1: Get manifest URL from GitHub
    println!("[1/4] Fetching manifest URL from GitHub...");
    let manifest_url = get_manifest_url(version).await?;
    println!("       Found manifest URL");

    // Step 2: Download manifest
    println!("[2/4] Downloading RMAN manifest...");
    let manifest_response = reqwest::get(&manifest_url).await?;
    let manifest_bytes = manifest_response.bytes().await?;
    println!("       Downloaded {} bytes", manifest_bytes.len());

    // Step 3: Parse manifest and find binary
    println!("[3/4] Parsing manifest...");
    let mut manifest_reader = Cursor::new(manifest_bytes);
    let manifest = rman::Manifest::read(&mut manifest_reader)?;

    let target_file = manifest
        .files
        .iter()
        .find(|f| f.name == TARGET_BINARY)
        .ok_or_else(|| format!("Binary '{}' not found in manifest", TARGET_BINARY))?;

    println!("       Found binary: {}", TARGET_BINARY);
    println!("       Chunks: {}", target_file.chunks.len());

    // Step 4: Download binary
    println!("[4/4] Downloading binary chunks from CDN...");

    // Ensure output directory exists
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let mut output_file = std::fs::File::create(output)?;

    target_file
        .download_all()
        .download(&mut ureq::Agent::new(), CDN_URL, &mut output_file)?;

    // Get file size
    let metadata = std::fs::metadata(output)?;
    let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);

    println!();
    println!("Download complete!");
    println!("  File: {}", output.display());
    println!("  Size: {:.2} MB", size_mb);
    println!();
    println!("You can now open this file in IDA, Ghidra, or another disassembler.");
    println!("Note: This is a Mach-O binary (macOS x86_64).");

    Ok(())
}

async fn get_manifest_url(version: &str) -> Result<String, Box<dyn std::error::Error>> {
    let octocrab = octocrab::Octocrab::default();

    let contents = octocrab
        .repos(GITHUB_OWNER, GITHUB_REPO)
        .get_content()
        .path(MANIFEST_PATH)
        .send()
        .await?;

    // Find the version file
    let filename = format!("{}.txt", version);
    let version_file = contents
        .items
        .iter()
        .find(|item| item.name == filename)
        .ok_or_else(|| format!("Version {} not found. Use --list to see available versions.", version))?;

    let download_url = version_file
        .download_url
        .as_ref()
        .ok_or("No download URL for version file")?;

    // Fetch the manifest URL from the file content
    let content = reqwest::get(download_url.as_str()).await?;
    let manifest_url = content.text().await?;

    Ok(manifest_url.trim().to_string())
}
