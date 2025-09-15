# LoL Meta Classes Automation Setup

This document explains how to set up automated synchronization with the `riot-manifests` repository using GitHub Actions.

## Overview

The automation system monitors the `Morilli/riot-manifests` repository for updates and automatically runs your sync script when new League of Legends client versions are detected.

## Setup Options

### Option 1: Repository Dispatch (Recommended)

This approach requires the `riot-manifests` repository to send a webhook to your repository when it updates.

#### In the riot-manifests repository:
Add this GitHub Action to `.github/workflows/notify-dependents.yml`:

```yaml
name: Notify Dependent Repositories

on:
  push:
    branches: [ main, master ]
    paths: 
      - 'LoL/**/*.txt'  # Only trigger on manifest file changes

jobs:
  notify:
    runs-on: ubuntu-latest
    steps:
    - name: Repository Dispatch
      uses: peter-evans/repository-dispatch@v3
      with:
        token: ${{ secrets.REPO_DISPATCH_TOKEN }}
        repository: YourUsername/lol-meta-classes  # Update with your repo
        event-type: manifest-updated
        client-payload: |
          {
            "ref": "${{ github.ref }}",
            "sha": "${{ github.sha }}",
            "repository": "${{ github.repository }}"
          }
```

#### Required Secrets:
1. In the `riot-manifests` repository, add a secret `REPO_DISPATCH_TOKEN`
2. This should be a GitHub Personal Access Token with `repo` scope
3. Generate it at: https://github.com/settings/tokens

### Option 2: Scheduled Polling (Fallback)

The workflow automatically runs every 6 hours to check for updates. No additional setup required.

### Option 3: Manual Triggers

You can manually trigger the sync from the GitHub Actions tab in your repository.

## Workflow Files Created

1. **`.github/workflows/sync-on-manifest-update.yml`** - Main automation workflow
2. **`.github/workflows/manual-sync.yml`** - Manual trigger with options

## Features

### Automatic Detection
- ✅ Detects new LoL client versions
- ✅ Downloads and processes manifest files
- ✅ Runs the dumper tool to extract class metadata
- ✅ Commits changes to the repository

### Smart Commits
- Only commits when actual changes are detected
- Includes version information in commit messages
- Automatically creates releases for major version updates

### Artifacts & Logging
- Uploads dump files as artifacts
- Preserves logs for debugging
- 30-day retention for artifacts

### Caching
- Caches Rust dependencies for faster builds
- Reduces build time from ~10 minutes to ~2 minutes

## Monitoring

### Check Workflow Status
1. Go to the "Actions" tab in your GitHub repository
2. Monitor the "Sync LoL Meta Classes" workflow
3. Check logs for any errors

### Notifications
- GitHub will email you if workflows fail
- You can set up Slack/Discord notifications using webhooks

## Testing

### Test the Workflow
1. Go to Actions → "Manual LoL Meta Sync"
2. Click "Run workflow"
3. Optionally specify a version or region
4. Monitor the execution

### Local Testing
```bash
# Build the dumper
cargo build --release --bin dumper --target x86_64-unknown-linux-gnu

# Run the sync locally
cargo run --release --bin meta-sync
```

## Troubleshooting

### Common Issues

1. **Build Failures**
   - Check Rust toolchain compatibility
   - Verify all dependencies are available
   - Review error logs in Actions tab

2. **Dumper Not Found**
   - Ensure the dumper binary is built correctly
   - Check the path in `execute_dumper()` function
   - Verify target architecture matches

3. **GitHub API Rate Limits**
   - The script uses public API endpoints (no auth required)
   - Rate limits are per-IP, usually not an issue for GitHub Actions

4. **Network Issues**
   - Riot CDN may be temporarily unavailable
   - Retry logic is built into the workflow

### Debug Mode
Set `RUST_LOG=debug` in the workflow environment variables for detailed logging.

## Security Notes

- No secrets are required for the basic sync functionality
- Repository dispatch requires a PAT with minimal `repo` scope
- All downloads are from official Riot CDN endpoints
- Workflow runs in isolated GitHub Actions environment

## Customization

### Change Sync Frequency
Edit the cron schedule in `sync-on-manifest-update.yml`:
```yaml
schedule:
  - cron: '0 */6 * * *'  # Every 6 hours
  # - cron: '0 0 * * *'   # Daily at midnight
  # - cron: '0 */2 * * *' # Every 2 hours
```

### Different Regions
Modify the `find_lol_game_client_directories()` function in `main.rs` to target different regions:
```rust
let path = "LoL/NA1/macos/lol-game-client";  // North America
let path = "LoL/KR/macos/lol-game-client";   // Korea
```

### Filter Specific Versions
Add version filtering logic in the sync script or use the manual workflow with version input.

## Docker Support

A Dockerfile is provided for containerized execution:

```bash
# Build the container
docker build -t lol-meta-sync .

# Run the sync
docker run -v $(pwd)/dumps:/app/dumps lol-meta-sync
```

This is useful for local development or running on other CI/CD platforms.
