//! Download the latest (or specified) LoL binary and run the dumper against it.
//!
//! Use this to locally test the dumper output and diagnose issues like oversized dumps.
//!
//! ## Usage
//!
//! ```bash
//! # Test the latest version
//! cargo run --release --bin test-dump
//!
//! # Test a specific version
//! cargo run --release --bin test-dump -- 16.7.7584427
//!
//! # Override dumper path
//! DUMPER_PATH=/path/to/dumper cargo run --release --bin test-dump
//! ```

use clap::Parser;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;

const CDN_URL: &str = "http://lol.secure.dyn.riotcdn.net/channels/public/bundles";
const GITHUB_OWNER: &str = "Morilli";
const GITHUB_REPO: &str = "riot-manifests";
const MANIFEST_PATH: &str = "LoL/EUW1/macos/lol-game-client";
const TARGET_BINARY: &str = "LeagueofLegends.app/Contents/MacOS/LeagueofLegends";

#[derive(Parser)]
#[command(name = "test-dump")]
#[command(about = "Download a LoL binary and run the dumper for local testing")]
struct Args {
    /// Version to test (defaults to latest available)
    #[arg(value_name = "VERSION")]
    version: Option<String>,

    /// Path to dumper binary (defaults to DUMPER_PATH env or target/release/dumper)
    #[arg(short, long, value_name = "PATH")]
    dumper: Option<PathBuf>,

    /// Keep temporary files after completion
    #[arg(long)]
    keep: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Resolve dumper path
    let dumper_path = args
        .dumper
        .or_else(|| std::env::var("DUMPER_PATH").ok().map(PathBuf::from))
        .unwrap_or_else(|| {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("../../target/release/dumper");
            p
        });

    if !dumper_path.exists() {
        eprintln!("ERROR: Dumper not found at: {}", dumper_path.display());
        eprintln!("Build it first: cargo build --release --bin dumper");
        std::process::exit(1);
    }
    eprintln!("Using dumper: {}", dumper_path.display());

    // Resolve version
    let version = match args.version {
        Some(v) => v,
        None => {
            eprintln!("[1/5] Fetching latest version from GitHub...");
            get_latest_version().await?
        }
    };
    eprintln!("Target version: {}", version);

    // Set up temp directory
    let temp_dir = PathBuf::from("temp");
    std::fs::create_dir_all(&temp_dir)?;
    let binary_path = temp_dir.join(&version);
    let output_path = PathBuf::from(format!("dumps/{}.json", version));

    // Download binary if not already cached
    if binary_path.exists() {
        eprintln!("[2/5] Binary already cached at {}", binary_path.display());
    } else {
        eprintln!("[2/5] Fetching manifest URL from GitHub...");
        let manifest_url = get_manifest_url(&version).await?;

        eprintln!("[3/5] Downloading RMAN manifest...");
        let manifest_response = reqwest::get(&manifest_url).await?;
        let manifest_bytes = manifest_response.bytes().await?;

        let mut manifest_reader = Cursor::new(manifest_bytes);
        let manifest = rman::Manifest::read(&mut manifest_reader)?;

        let target_file = manifest
            .files
            .iter()
            .find(|f| f.name == TARGET_BINARY)
            .ok_or_else(|| format!("Binary '{}' not found in manifest", TARGET_BINARY))?;

        eprintln!(
            "[3/5] Downloading binary ({} chunks)...",
            target_file.chunks.len()
        );

        let mut output_file = std::fs::File::create(&binary_path)?;
        target_file
            .download_all()
            .download(&mut ureq::Agent::new(), CDN_URL, &mut output_file)?;

        let size_mb = std::fs::metadata(&binary_path)?.len() as f64 / (1024.0 * 1024.0);
        eprintln!("       Binary: {:.1} MB", size_mb);
    }

    // Run dumper
    eprintln!("[4/5] Running dumper...");
    std::fs::create_dir_all("dumps")?;

    let status = Command::new(&dumper_path)
        .arg(&binary_path)
        .arg("-o")
        .arg(&output_path)
        .status()?;

    if !status.success() {
        eprintln!("ERROR: Dumper exited with status: {}", status);
        std::process::exit(1);
    }

    // Validate output
    eprintln!("[5/5] Validating output...");
    let output_meta = std::fs::metadata(&output_path)?;
    let size_mb = output_meta.len() as f64 / (1024.0 * 1024.0);
    eprintln!("       Output: {} ({:.2} MB)", output_path.display(), size_mb);

    if size_mb > 50.0 {
        eprintln!(
            "WARNING: Output is {:.1} MB - this is abnormally large! Expected ~7 MB.",
            size_mb
        );
        eprintln!("         The dump is likely corrupted (runaway list serialization).");
        std::process::exit(1);
    } else {
        eprintln!("       Size looks normal (expected ~7 MB).");
    }

    // Cleanup
    if !args.keep {
        eprintln!("Cleaning up binary (use --keep to retain)...");
        let _ = std::fs::remove_file(&binary_path);
    }

    eprintln!("Done! Output: {}", output_path.display());
    Ok(())
}

async fn get_latest_version() -> Result<String, Box<dyn std::error::Error>> {
    let octocrab = octocrab::Octocrab::default();

    let contents = octocrab
        .repos(GITHUB_OWNER, GITHUB_REPO)
        .get_content()
        .path(MANIFEST_PATH)
        .send()
        .await?;

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

    versions.sort_by(|a, b| {
        let a_ver = semver::Version::parse(a).ok();
        let b_ver = semver::Version::parse(b).ok();
        a_ver.cmp(&b_ver)
    });

    versions
        .last()
        .cloned()
        .ok_or_else(|| "No versions found".into())
}

async fn get_manifest_url(version: &str) -> Result<String, Box<dyn std::error::Error>> {
    let octocrab = octocrab::Octocrab::default();

    let contents = octocrab
        .repos(GITHUB_OWNER, GITHUB_REPO)
        .get_content()
        .path(MANIFEST_PATH)
        .send()
        .await?;

    let filename = format!("{}.txt", version);
    let version_file = contents
        .items
        .iter()
        .find(|item| item.name == filename)
        .ok_or_else(|| {
            format!(
                "Version {} not found. Use download-binary --list to see available versions.",
                version
            )
        })?;

    let download_url = version_file
        .download_url
        .as_ref()
        .ok_or("No download URL for version file")?;

    let content = reqwest::get(download_url.as_str()).await?;
    let manifest_url = content.text().await?;

    Ok(manifest_url.trim().to_string())
}
