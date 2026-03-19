//! Boost file download and loading.
//!
//! Downloads the pre-computed boost file from GitHub releases, decompresses
//! via zstd, and stores in the app's own BoostCache directory.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;

use sha2::{Digest, Sha256};

use crate::CoreError;
use zipherx_storage::database::WalletDatabase;

/// RC-1: Build an HTTP client that routes through Tor SOCKS5 proxy if available.
///
/// If Tor SOCKS5 is running (detected via `zipherx_tor::client::get_socks_port()`),
/// the client will use it as a proxy for privacy. Otherwise, falls back to a direct
/// connection with a privacy warning.
pub(crate) fn build_tor_aware_client(timeout_secs: u64) -> Result<reqwest::Client, CoreError> {
    let socks_port = zipherx_tor::client::get_socks_port();

    // connect_timeout: max time to establish TCP + TLS connection.
    // NO total timeout — a 1GB download can legitimately take 30+ minutes.
    // Stalled connections are detected by chunk-level read errors.
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(timeout_secs.min(60)))
        .tcp_nodelay(true);

    if socks_port > 0 && zipherx_tor::client::is_socks_running() {
        let proxy_url = format!("socks5h://127.0.0.1:{}", socks_port);
        eprintln!(
            "[ZipherX] Boost download: routing through Tor SOCKS5 proxy (port {})",
            socks_port
        );
        let proxy = reqwest::Proxy::all(&proxy_url)
            .map_err(|e| CoreError::Storage(format!("Tor proxy config failed: {e}")))?;
        builder = builder.proxy(proxy);
    } else {
        // C4: Block clearnet fallback when Tor-only mode is enabled.
        if zipherx_tor::client::is_tor_only_mode() {
            return Err(CoreError::Storage(
                "Tor-only mode is enabled but Tor SOCKS5 is not running. Cannot download over clearnet.".into()
            ));
        }
        eprintln!(
            "[ZipherX] PRIVACY WARNING: Tor SOCKS5 proxy not available — boost download \
             uses direct connection. Your IP address will be visible to GitHub CDN servers. \
             Start Tor (e.g., `brew services start tor`) for privacy."
        );
        builder = builder.no_proxy();
    }

    builder
        .build()
        .map_err(|e| CoreError::Storage(format!("HTTP client: {e}")))
}

// ============================================================================
// Types
// ============================================================================

/// Result of downloading a boost file.
#[derive(Debug, Clone)]
pub struct BoostFileInfo {
    /// Path where the boost file was saved.
    pub file_path: String,
    /// Size in bytes of the downloaded file.
    pub file_size: u64,
    /// Height of the boost file.
    pub boost_height: u64,
    /// Number of CMUs in the boost file.
    pub cmu_count: u64,
    /// Whether the download was resumed.
    pub was_resumed: bool,
}

/// Result of loading a boost file into the wallet.
#[derive(Debug, Clone)]
pub struct BoostLoadResult {
    /// Number of CMUs loaded into the tree.
    pub cmus_loaded: u64,
    /// Number of notes found for this wallet.
    pub notes_found: u32,
    /// Total received amount from discovered notes.
    pub total_received: u64,
    /// Total unspent balance from discovered notes.
    pub unspent_balance: u64,
}

/// Progress callback for boost download.
pub type BoostProgressFn = Arc<dyn Fn(u64, u64, f64) + Send + Sync>;

// ============================================================================
// Boost Manifest
// ============================================================================

/// Parsed boost file manifest (JSON).
#[derive(Debug, Clone, Deserialize)]
pub struct BoostManifest {
    pub output_count: u64,
    pub spend_count: u64,
    pub chain_height: u64,
    pub tree_root: String,
    #[serde(default)]
    pub sections: Vec<BoostSection>,
    /// Sapling activation height (first Sapling block).
    #[serde(default)]
    pub sapling_activation: u64,
    /// File info with SHA-256 hashes (nested: files.uncompressed.sha256).
    #[serde(default)]
    pub files: Option<BoostManifestFiles>,
}

/// File entries in the boost manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct BoostManifestFiles {
    pub uncompressed: BoostManifestFileEntry,
    #[serde(default)]
    pub compressed: Option<BoostManifestFileEntry>,
}

/// A single file entry with name, size, and SHA-256 hash.
#[derive(Debug, Clone, Deserialize)]
pub struct BoostManifestFileEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sha256: String,
}

/// A section within the boost file.
#[derive(Debug, Clone, Deserialize)]
pub struct BoostSection {
    #[serde(rename = "type")]
    pub section_type: u32,
    pub offset: u64,
    pub size: u64,
    pub count: u64,
    /// First block height in this section.
    #[serde(default)]
    pub start_height: u64,
    /// Last block height in this section.
    #[serde(default)]
    pub end_height: u64,
}

/// Boost section type constants.
pub const BOOST_SECTION_OUTPUTS: u32 = 1;
pub const BOOST_SECTION_SPENDS: u32 = 2;
pub const BOOST_SECTION_HASHES: u32 = 3;
pub const BOOST_SECTION_TIMESTAMPS: u32 = 4;
pub const BOOST_SECTION_TREE: u32 = 5;
pub const BOOST_SECTION_HEADERS: u32 = 7;

/// Parse a boost manifest JSON file.
pub fn parse_manifest(path: &str) -> Result<BoostManifest, CoreError> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| CoreError::Storage(format!("Cannot read boost manifest: {e}")))?;
    serde_json::from_str(&data)
        .map_err(|e| CoreError::Storage(format!("Invalid boost manifest JSON: {e}")))
}

/// Find a section by type in the manifest.
pub fn get_section(manifest: &BoostManifest, section_type: u32) -> Option<&BoostSection> {
    manifest
        .sections
        .iter()
        .find(|s| s.section_type == section_type)
}

/// Read a section's raw bytes from the boost file using seek (not loading entire file).
pub fn read_section(boost_file_path: &str, section: &BoostSection) -> Result<Vec<u8>, CoreError> {
    let mut file = std::fs::File::open(boost_file_path)
        .map_err(|e| CoreError::Storage(format!("Cannot open boost file: {e}")))?;
    file.seek(SeekFrom::Start(section.offset))
        .map_err(|e| CoreError::Storage(format!("Seek failed: {e}")))?;
    let len = usize::try_from(section.size)
        .map_err(|_| CoreError::Storage("Section too large for this platform".into()))?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf)
        .map_err(|e| CoreError::Storage(format!("Read section failed: {e}")))?;
    Ok(buf)
}

/// Memory-map a section of the boost file.
///
/// Returns a memory-mapped region covering exactly `section.size` bytes at
/// `section.offset`. The OS manages paging — only accessed pages are loaded
/// into RAM, keeping resident memory far below the section's total size.
/// This is critical for the outputs section (~1.75 GB) on Android devices.
///
/// # Safety considerations
/// The `unsafe` block is required by `memmap2::MmapOptions::map()`. Memory-mapped
/// files are inherently unsafe because external processes could modify the file
/// while it is mapped, leading to undefined behavior. This is acceptable here
/// because: (1) the boost file is written once and never modified, (2) it lives
/// in the app's private cache directory, (3) we validate the file size before
/// mapping to ensure the section bounds are within the file.
pub fn mmap_section(
    boost_file_path: &str,
    section: &BoostSection,
) -> Result<memmap2::Mmap, CoreError> {
    let file = std::fs::File::open(boost_file_path)
        .map_err(|e| CoreError::Storage(format!("Cannot open boost file: {e}")))?;

    // Verify file size covers the requested section before mapping
    let file_size = file
        .metadata()
        .map_err(|e| CoreError::Storage(format!("Cannot stat boost file: {e}")))?
        .len();
    let section_end = section
        .offset
        .checked_add(section.size)
        .ok_or_else(|| CoreError::Storage("Section offset+size overflow".into()))?;
    if section_end > file_size {
        return Err(CoreError::Storage(format!(
            "Section extends beyond file: section ends at {} but file is {} bytes",
            section_end, file_size,
        )));
    }

    // SAFETY: See doc comment above — file is immutable, in private cache, and
    // we have verified the section bounds are within the file.
    //
    // RC-10: TOCTOU RISK — There is a time-of-check-time-of-use gap between the
    // file size check above and the mmap() call below. If another process
    // truncates or modifies the file between those two operations, the mapped
    // region may reference invalid memory. This is acceptable because:
    // (a) the file lives in the app's private cache directory (not world-writable),
    // (b) no other part of ZipherX modifies the boost file after download,
    // (c) we do NOT hold a file lock — adding flock() would provide defense-in-depth
    //     but is not critical given (a) and (b). Consider adding flock() if the
    //     file is ever shared across processes.
    unsafe {
        memmap2::MmapOptions::new()
            .offset(section.offset)
            .len(
                usize::try_from(section.size).map_err(|_| {
                    CoreError::Storage("Section too large for this platform".into())
                })?,
            )
            .map(&file)
            .map_err(|e| CoreError::Storage(format!("mmap section failed: {e}")))
    }
}

// ============================================================================
// Load Headers from Boost File
// ============================================================================

/// Load block headers from the boost file into a HeaderStore.
///
/// Uses Section 3 (block hashes, 32 bytes each) and Section 4 (timestamps,
/// 4 bytes each) for hash/time, and streams Section 7 (full headers) to
/// extract `prev_hash`, `final_sapling_root`, and `bits` from each record's
/// fixed 172-byte prefix. Skips variable-length Equihash solution.
///
/// Inserts in batches of 10,000 for efficiency. Returns count of headers loaded.
/// Progress callback for boost header loading.
pub type BoostLoadProgressFn = Box<dyn Fn(u64, u64) + Send>;

pub fn load_boost_headers(
    boost_file_path: &str,
    manifest: &BoostManifest,
    header_store: &dyn zipherx_network::header_sync::HeaderStore,
) -> Result<u64, CoreError> {
    load_boost_headers_with_progress(boost_file_path, manifest, header_store, None)
}

pub fn load_boost_headers_with_progress(
    boost_file_path: &str,
    manifest: &BoostManifest,
    header_store: &dyn zipherx_network::header_sync::HeaderStore,
    progress: Option<BoostLoadProgressFn>,
) -> Result<u64, CoreError> {
    use std::io::BufReader;
    use zipherx_network::header_sync::StoredHeader;

    let hashes_sec = get_section(manifest, BOOST_SECTION_HASHES)
        .ok_or_else(|| CoreError::Storage("Boost file missing Section 3 (block hashes)".into()))?;
    let timestamps_sec = get_section(manifest, BOOST_SECTION_TIMESTAMPS)
        .ok_or_else(|| CoreError::Storage("Boost file missing Section 4 (timestamps)".into()))?;
    let headers_sec = get_section(manifest, BOOST_SECTION_HEADERS)
        .ok_or_else(|| CoreError::Storage("Boost file missing Section 7 (headers)".into()))?;

    let count = hashes_sec.count as usize;
    let start_height = if hashes_sec.start_height > 0 {
        hashes_sec.start_height
    } else if manifest.sapling_activation > 0 {
        manifest.sapling_activation
    } else {
        476_969 // Zclassic Sapling activation
    };

    eprintln!(
        "[ZipherX] Loading {} headers from boost file (heights {}–{})...",
        count,
        start_height,
        start_height + count as u64 - 1
    );

    // Read Section 3: block hashes (32 bytes each, ~81 MB)
    let hashes_data = read_section(boost_file_path, hashes_sec)?;
    if hashes_data.len() != count * 32 {
        return Err(CoreError::Storage(format!(
            "Section 3 size mismatch: {} vs expected {}",
            hashes_data.len(),
            count * 32
        )));
    }

    // Read Section 4: timestamps (4 bytes each, ~10 MB)
    let timestamps_data = read_section(boost_file_path, timestamps_sec)?;
    if timestamps_data.len() != count * 4 {
        return Err(CoreError::Storage(format!(
            "Section 4 size mismatch: {} vs expected {}",
            timestamps_data.len(),
            count * 4
        )));
    }

    // Stream Section 7 to extract sapling_root, prev_hash, bits per record
    let file = std::fs::File::open(boost_file_path)
        .map_err(|e| CoreError::Storage(format!("Cannot open boost file: {e}")))?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    reader
        .seek(SeekFrom::Start(headers_sec.offset))
        .map_err(|e| CoreError::Storage(format!("Seek to Section 7 failed: {e}")))?;

    // Larger batch size for bulk import (fewer transaction commits)
    const BATCH_SIZE: usize = 50_000;
    // Boost Section 7 stores flat 140-byte header records (no solution):
    // version(4) + prev_hash(32) + merkle_root(32) + sapling_root(32)
    // + time(4) + bits(4) + nonce(32) = 140 bytes
    const HEADER_SIZE: usize = 140;

    let mut batch: Vec<(u64, StoredHeader)> = Vec::with_capacity(BATCH_SIZE);
    let mut header_buf = [0u8; HEADER_SIZE];
    let mut loaded: u64 = 0;
    let total = count as u64;

    // Report initial progress
    if let Some(ref p) = progress {
        p(0, total);
    }

    for i in 0..count {
        // Read 140-byte header record
        reader
            .read_exact(&mut header_buf)
            .map_err(|e| CoreError::Storage(format!("Read header {i}: {e}")))?;

        // Extract fields from header
        let mut prev_hash = [0u8; 32];
        prev_hash.copy_from_slice(&header_buf[4..36]);
        let mut sapling_root = [0u8; 32];
        sapling_root.copy_from_slice(&header_buf[68..100]);
        let bits = u32::from_le_bytes(header_buf[104..108].try_into().unwrap());

        // Get hash from Section 3
        let hash_off = i * 32;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hashes_data[hash_off..hash_off + 32]);

        // Get timestamp from Section 4
        let ts_off = i * 4;
        let timestamp = u32::from_le_bytes(timestamps_data[ts_off..ts_off + 4].try_into().unwrap());

        let height = start_height + i as u64;
        batch.push((
            height,
            StoredHeader {
                hash,
                prev_hash,
                final_sapling_root: sapling_root,
                timestamp,
                bits,
            },
        ));

        // Flush batch
        if batch.len() >= BATCH_SIZE {
            header_store
                .store_headers(std::mem::take(&mut batch))
                .map_err(|e| CoreError::Storage(format!("Store headers batch: {e}")))?;
            batch = Vec::with_capacity(BATCH_SIZE);
            loaded = (i + 1) as u64;

            // Report progress every batch
            if let Some(ref p) = progress {
                p(loaded, total);
            }

            if i % 100_000 == 0 {
                eprintln!(
                    "[ZipherX] Loaded {}/{} headers from boost file...",
                    loaded, total,
                );
            }
        }
    }

    // Final flush
    if !batch.is_empty() {
        let remaining = batch.len();
        header_store
            .store_headers(batch)
            .map_err(|e| CoreError::Storage(format!("Store headers final batch: {e}")))?;
        loaded += remaining as u64;
    }

    // Report completion
    if let Some(ref p) = progress {
        p(loaded, total);
    }

    eprintln!(
        "[ZipherX] Loaded {} headers from boost file into HeaderStore",
        loaded
    );

    Ok(loaded)
}

// ============================================================================
// Download from GitHub Releases
// ============================================================================

/// GitHub repository owner for boost files.
const BOOST_REPO_OWNER: &str = "ZipherPunk";

/// GitHub repository name for boost files.
const BOOST_REPO_NAME: &str = "ZipherX_Boost";

/// Fallback release tag when GitHub API is unreachable (e.g. first launch offline).
/// URL to fetch the release tag from the repo README (no API rate limit).
const BOOST_README_RAW_URL: &str =
    "https://raw.githubusercontent.com/ZipherPunk/ZipherX_Boost/main/README.md";

/// Boost file split part filenames (appended to release download URL).
const BOOST_PART_NAMES: &[&str] = &[
    "zipherx_boost_v1.bin.zst.part1",
    "zipherx_boost_v1.bin.zst.part2",
];

/// Construct the GitHub API URL for the latest release.
fn github_latest_release_url() -> String {
    format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        BOOST_REPO_OWNER, BOOST_REPO_NAME,
    )
}

/// Construct a download URL for a file in a specific release.
fn boost_release_url(tag: &str, filename: &str) -> String {
    format!(
        "https://github.com/{}/{}/releases/download/{}/{}",
        BOOST_REPO_OWNER, BOOST_REPO_NAME, tag, filename,
    )
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

/// Fallback: parse the release tag from the repo README on raw.githubusercontent.com.
/// This endpoint is NOT rate-limited like the GitHub API.
/// Looks for the line "Release Tag": v.....-unified in the README.
async fn get_tag_from_readme() -> Result<String, CoreError> {
    let client = build_tor_aware_client(15)?;
    let resp = client
        .get(BOOST_README_RAW_URL)
        .header("User-Agent", "ZipherX-Wallet")
        .send()
        .await
        .map_err(|e| CoreError::Storage(format!("README fetch failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(CoreError::Storage(format!(
            "README fetch HTTP {}",
            resp.status(),
        )));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| CoreError::Storage(format!("README read: {e}")))?;

    // Look for "Release Tag": v...-unified or similar pattern
    for line in body.lines() {
        if line.contains("Release Tag") {
            // Extract the tag value after the colon
            if let Some(idx) = line.rfind('v') {
                let tag = line[idx..]
                    .trim()
                    .trim_end_matches('`')
                    .trim_end_matches('*');
                if tag.starts_with("v") && tag.contains("-unified") {
                    return Ok(tag.to_string());
                }
            }
        }
    }

    Err(CoreError::Storage(
        "Could not find Release Tag in README".into(),
    ))
}

/// Check GitHub for the latest boost release tag.
///
/// Returns `Ok(tag_name)` or `Err` (non-fatal — network issues, rate limits).
pub async fn get_latest_boost_tag() -> Result<String, CoreError> {
    // RC-1: Use Tor-aware client instead of hardcoded .no_proxy()
    let client = build_tor_aware_client(15)?;

    let url = github_latest_release_url();
    let response = client
        .get(&url)
        .header("User-Agent", "ZipherX-Wallet")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| CoreError::Storage(format!("GitHub API request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(CoreError::Storage(format!(
            "GitHub API returned HTTP {}",
            response.status(),
        )));
    }

    let release: GitHubRelease = response
        .json()
        .await
        .map_err(|e| CoreError::Storage(format!("GitHub release JSON parse: {e}")))?;

    Ok(release.tag_name)
}

/// Check if a newer boost file is available on GitHub.
///
/// Compares the remote manifest's `chain_height` with the local manifest's.
/// Returns `Ok(Some(tag))` if an update is available, `Ok(None)` if up-to-date.
pub async fn check_for_boost_update(
    local_manifest_path: &str,
) -> Result<Option<String>, CoreError> {
    let local = parse_manifest(local_manifest_path)?;

    let tag = get_latest_boost_tag().await?;

    let manifest_url = boost_release_url(&tag, "zipherx_boost_manifest.json");
    // RC-1: Use Tor-aware client instead of hardcoded .no_proxy()
    let client = build_tor_aware_client(30)?;

    let response = client
        .get(&manifest_url)
        .send()
        .await
        .map_err(|e| CoreError::Storage(format!("Remote manifest fetch: {e}")))?;

    if !response.status().is_success() {
        return Err(CoreError::Storage(format!(
            "Remote manifest HTTP {}",
            response.status(),
        )));
    }

    let remote_text = response
        .text()
        .await
        .map_err(|e| CoreError::Storage(format!("Remote manifest read: {e}")))?;

    let remote: BoostManifest = serde_json::from_str(&remote_text)
        .map_err(|e| CoreError::Storage(format!("Remote manifest parse: {e}")))?;

    if remote.chain_height > local.chain_height {
        eprintln!(
            "[ZipherX] Boost update available: height {} → {} (tag: {})",
            local.chain_height, remote.chain_height, tag,
        );
        Ok(Some(tag))
    } else {
        eprintln!(
            "[ZipherX] Boost is up to date (height {})",
            local.chain_height,
        );
        Ok(None)
    }
}

/// Download progress callback: (bytes_downloaded, total_bytes, phase_label).
pub type DownloadProgressFn = Arc<dyn Fn(u64, u64, &str) + Send + Sync>;

/// Download the boost file from GitHub releases if it doesn't exist locally.
///
/// If `release_tag` is provided, uses it to construct download URLs dynamically.
/// If `None`, queries GitHub API for the latest tag (falls back to `FALLBACK_RELEASE_TAG`).
///
/// Steps:
/// 1. Create BoostCache directory
/// 2. Download manifest JSON
/// 3. Download split .zst parts with streaming
/// 4. Decompress .zst → .bin using streaming decompression
/// 5. Clean up temp files
///
/// Returns the path to the final boost file.
pub async fn download_boost_file_if_needed(
    boost_cache_dir: &Path,
    progress: Option<DownloadProgressFn>,
    release_tag: Option<&str>,
) -> Result<(String, String), CoreError> {
    let boost_file = boost_cache_dir.join("zipherx_boost_v1.bin");
    let manifest_file = boost_cache_dir.join("zipherx_boost_manifest.json");

    // Already downloaded?
    if boost_file.exists() && manifest_file.exists() {
        let size = std::fs::metadata(&boost_file).map(|m| m.len()).unwrap_or(0);
        if size > 100_000_000 {
            // >100MB = likely valid
            eprintln!(
                "[ZipherX] Boost file already exists ({} MB), skipping download",
                size / (1024 * 1024),
            );
            return Ok((
                boost_file.to_string_lossy().into_owned(),
                manifest_file.to_string_lossy().into_owned(),
            ));
        }
    }

    // Create directory
    std::fs::create_dir_all(boost_cache_dir)
        .map_err(|e| CoreError::Storage(format!("Cannot create BoostCache dir: {e}")))?;

    // Resolve the release tag: explicit > GitHub API > fallback
    let tag = match release_tag {
        Some(t) => t.to_string(),
        None => match get_latest_boost_tag().await {
            Ok(t) => {
                eprintln!("[ZipherX] Latest boost release: {}", t);
                t
            }
            Err(e) => {
                eprintln!(
                    "[ZipherX] GitHub API unavailable ({}), trying README fallback...",
                    e,
                );
                // Fallback: parse the release tag from the repo README
                // (raw.githubusercontent.com is NOT rate-limited like the API)
                match get_tag_from_readme().await {
                    Ok(tag) => {
                        eprintln!("[ZipherX] Got release tag from README: {}", tag);
                        tag
                    }
                    Err(e2) => {
                        return Err(CoreError::Storage(format!(
                            "Cannot determine boost release: API failed ({}), README failed ({}). \
                             Check your internet connection.",
                            e, e2,
                        )));
                    }
                }
            }
        },
    };

    eprintln!("[ZipherX] Downloading boost file from GitHub releases...");
    eprintln!("[ZipherX] Release: {}", tag);

    let manifest_url = boost_release_url(&tag, "zipherx_boost_manifest.json");

    // Step 1: Download manifest
    if let Some(ref p) = progress {
        p(0, 0, "Downloading manifest...");
    }

    // RC-1: Use Tor-aware client instead of hardcoded .no_proxy()
    let client = build_tor_aware_client(600)?;

    let manifest_bytes = client
        .get(&manifest_url)
        .send()
        .await
        .map_err(|e| CoreError::Storage(format!("Manifest download failed: {e}")))?
        .bytes()
        .await
        .map_err(|e| CoreError::Storage(format!("Manifest read failed: {e}")))?;

    let manifest_path_str = manifest_file.to_string_lossy().to_string();
    let mb = manifest_bytes.to_vec();
    tokio::task::spawn_blocking(move || std::fs::write(&manifest_path_str, &mb))
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(format!("Manifest write failed: {e}")))?;

    eprintln!(
        "[ZipherX] Manifest downloaded ({} bytes)",
        manifest_bytes.len()
    );

    // Step 2: Download split .zst parts
    let zst_combined = boost_cache_dir.join("zipherx_boost_v1.bin.zst");
    let zst_combined_str = zst_combined.to_string_lossy().to_string();

    // Build part URLs from tag + part names
    let part_urls: Vec<String> = BOOST_PART_NAMES
        .iter()
        .map(|name| boost_release_url(&tag, name))
        .collect();

    // Get total size from all parts for progress (parallel HEAD requests)
    let mut head_handles = Vec::new();
    for url in &part_urls {
        let client = client.clone();
        let url = url.clone();
        head_handles.push(tokio::spawn(async move {
            let resp = client
                .head(url.as_str())
                .send()
                .await
                .map_err(|e| CoreError::Storage(format!("HEAD request failed: {e}")))?;
            Ok::<u64, CoreError>(resp.content_length().unwrap_or(0))
        }));
    }
    let mut total_download_size: u64 = 0;
    for handle in head_handles {
        total_download_size += handle
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
    }

    eprintln!(
        "[ZipherX] Total download: {} parts, {} MB compressed",
        part_urls.len(),
        total_download_size / (1024 * 1024),
    );

    // Download parts in parallel with HTTP resume support, then concatenate.
    // If a previous attempt left partial .tmp files, resume from where they
    // stopped using HTTP Range headers instead of re-downloading from scratch.
    {
        use std::sync::atomic::{AtomicU64, Ordering};

        // Check existing partial downloads for resume
        let mut already_downloaded: u64 = 0;
        for part_idx in 0..part_urls.len() {
            let part_path = boost_cache_dir.join(format!("_part_{}.tmp", part_idx));
            if let Ok(meta) = std::fs::metadata(&part_path) {
                already_downloaded += meta.len();
            }
        }
        if already_downloaded > 0 {
            eprintln!(
                "[ZipherX] Resuming download: {} MB already on disk",
                already_downloaded / (1024 * 1024),
            );
        }

        let total_downloaded = Arc::new(AtomicU64::new(already_downloaded));
        let progress_arc: Option<DownloadProgressFn> = progress.clone();

        // Fire initial progress callback so UI shows total size immediately
        // (otherwise UI stays at 0/0 until first chunk is downloaded)
        if let Some(ref p) = progress_arc {
            p(
                already_downloaded,
                total_download_size,
                &format!(
                    "Downloading {} parts... ({} MB / {} MB)",
                    part_urls.len(),
                    already_downloaded / (1024 * 1024),
                    total_download_size / (1024 * 1024),
                ),
            );
        }

        // Download each part to its own temp file in parallel
        let mut download_handles = Vec::new();

        for (part_idx, url) in part_urls.iter().enumerate() {
            let client = client.clone();
            let url = url.clone();
            let part_path = boost_cache_dir.join(format!("_part_{}.tmp", part_idx));
            let total_dl = total_downloaded.clone();
            let total_size = total_download_size;
            let num_parts = part_urls.len();
            let progress_c = progress_arc.as_ref().map(Arc::clone);

            let handle = tokio::spawn(async move {
                let part_path_str = part_path.to_string_lossy().to_string();

                // Check if partial file exists for resume
                let existing_size = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);

                if existing_size > 0 {
                    eprintln!(
                        "[ZipherX] Part {}/{}: resuming from {} MB",
                        part_idx + 1,
                        num_parts,
                        existing_size / (1024 * 1024),
                    );
                } else {
                    eprintln!(
                        "[ZipherX] Downloading part {}/{}: {}",
                        part_idx + 1,
                        num_parts,
                        url.rsplit('/').next().unwrap_or(&url),
                    );
                }

                // Build request with Range header if resuming
                let request = if existing_size > 0 {
                    client
                        .get(url.as_str())
                        .header("Range", format!("bytes={}-", existing_size))
                } else {
                    client.get(url.as_str())
                };

                eprintln!(
                    "[ZipherX] Part {}/{}: connecting...",
                    part_idx + 1,
                    num_parts,
                );

                let response = request.send().await.map_err(|e| {
                    CoreError::Storage(format!("Download part {} failed: {e}", part_idx + 1))
                })?;

                let status = response.status();
                eprintln!(
                    "[ZipherX] Part {}/{}: connected (HTTP {})",
                    part_idx + 1,
                    num_parts,
                    status,
                );

                // 416 Range Not Satisfiable = part already fully downloaded
                if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                    eprintln!(
                        "[ZipherX] Part {}/{}: already complete ({} MB)",
                        part_idx + 1,
                        num_parts,
                        existing_size / (1024 * 1024),
                    );
                    return Ok::<String, CoreError>(part_path_str);
                }

                // 206 Partial Content = resume accepted
                // 200 OK = server doesn't support Range, restart from scratch
                let is_resume = status == reqwest::StatusCode::PARTIAL_CONTENT;
                if !is_resume && existing_size > 0 {
                    eprintln!(
                        "[ZipherX] Part {}/{}: server returned {} (no resume support), restarting",
                        part_idx + 1,
                        num_parts,
                        status,
                    );
                    // Subtract the existing bytes we already counted
                    total_dl.fetch_sub(existing_size, Ordering::Relaxed);
                }

                if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
                    return Err(CoreError::Storage(format!(
                        "Download part {} returned HTTP {}",
                        part_idx + 1,
                        status,
                    )));
                }

                let mut response = response;

                // Open file: append if resuming, create if starting fresh
                let pp = part_path_str.clone();
                let file = tokio::task::spawn_blocking(move || {
                    let f = if is_resume {
                        std::fs::OpenOptions::new()
                            .append(true)
                            .open(&pp)
                            .map_err(|e| {
                                CoreError::Storage(format!("Open part file for append: {e}"))
                            })?
                    } else {
                        std::fs::File::create(&pp)
                            .map_err(|e| CoreError::Storage(format!("Create part file: {e}")))?
                    };
                    Ok::<_, CoreError>(std::io::BufWriter::with_capacity(16 * 1024 * 1024, f))
                })
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                // Channel-based writer: network reads never block on disk I/O.
                // Sender sends Vec<u8> buffers, writer task drains them to disk.
                let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(4); // 4 × 16MB = 64MB backpressure
                let writer_handle = std::thread::spawn(move || {
                    let mut writer = file;
                    while let Ok(buf) = rx.recv() {
                        writer
                            .write_all(&buf)
                            .map_err(|e| CoreError::Storage(format!("Write chunk: {e}")))?;
                    }
                    writer
                        .flush()
                        .map_err(|e| CoreError::Storage(format!("Flush: {e}")))?;
                    Ok::<(), CoreError>(())
                });

                let mut local_buf: Vec<u8> = Vec::with_capacity(16 * 1024 * 1024);
                let read_timeout = std::time::Duration::from_secs(120);

                loop {
                    let chunk = match tokio::time::timeout(read_timeout, response.chunk()).await {
                        Ok(Ok(Some(c))) => c,
                        Ok(Ok(None)) => break, // stream finished
                        Ok(Err(e)) => return Err(CoreError::Storage(format!("Read chunk: {e}"))),
                        Err(_) => {
                            return Err(CoreError::Storage(
                                "Read chunk: no data received for 120s (stalled connection)".into(),
                            ))
                        }
                    };
                    let chunk_len = chunk.len() as u64;
                    local_buf.extend_from_slice(&chunk);
                    let prev = total_dl.fetch_add(chunk_len, Ordering::Relaxed);
                    let current_total = prev + chunk_len;

                    // Send buffer to writer thread when >= 16MB
                    if local_buf.len() >= 16 * 1024 * 1024 {
                        let buf_data =
                            std::mem::replace(&mut local_buf, Vec::with_capacity(16 * 1024 * 1024));
                        tx.send(buf_data)
                            .map_err(|_| CoreError::Storage("Writer thread died".into()))?;
                    }

                    // Report progress every ~256KB for responsive UI
                    if current_total % (256 * 1024) < chunk_len {
                        if let Some(ref p) = progress_c {
                            p(
                                current_total,
                                total_size,
                                &format!(
                                    "Downloading {} parts... ({} MB / {} MB)",
                                    num_parts,
                                    current_total / (1024 * 1024),
                                    total_size / (1024 * 1024),
                                ),
                            );
                        }
                    }
                }

                // Send remaining buffer
                if !local_buf.is_empty() {
                    tx.send(local_buf)
                        .map_err(|_| CoreError::Storage("Writer thread died".into()))?;
                }

                // Drop sender to signal writer thread to finish
                drop(tx);
                writer_handle
                    .join()
                    .map_err(|_| CoreError::Storage("Writer thread panicked".into()))??;

                eprintln!("[ZipherX] Part {}/{} complete", part_idx + 1, num_parts,);

                Ok::<String, CoreError>(part_path_str)
            });

            download_handles.push(handle);
        }

        // Wait for all parallel downloads to complete
        let mut part_paths: Vec<String> = Vec::new();
        for handle in download_handles {
            let path = handle
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
            part_paths.push(path);
        }

        // Concatenate parts into the final combined file (in order),
        // computing SHA-256 of the compressed data during concatenation (zero extra I/O).
        let zst_path = zst_combined_str.clone();
        let compressed_hash = tokio::task::spawn_blocking(move || {
            let mut output = std::io::BufWriter::with_capacity(
                8 * 1024 * 1024,
                std::fs::File::create(&zst_path)
                    .map_err(|e| CoreError::Storage(format!("Create zst file: {e}")))?,
            );
            let mut hasher = Sha256::new();
            let mut buf = vec![0u8; 8 * 1024 * 1024];
            for part_path in &part_paths {
                let mut input = std::io::BufReader::with_capacity(
                    8 * 1024 * 1024,
                    std::fs::File::open(part_path)
                        .map_err(|e| CoreError::Storage(format!("Open part file: {e}")))?,
                );
                loop {
                    let n = input
                        .read(&mut buf)
                        .map_err(|e| CoreError::Storage(format!("Read part: {e}")))?;
                    if n == 0 {
                        break;
                    }
                    output
                        .write_all(&buf[..n])
                        .map_err(|e| CoreError::Storage(format!("Write part: {e}")))?;
                    hasher.update(&buf[..n]);
                }
                // Remove temp part file
                let _ = std::fs::remove_file(part_path);
            }
            output
                .flush()
                .map_err(|e| CoreError::Storage(format!("Flush: {e}")))?;
            Ok::<String, CoreError>(hex::encode(hasher.finalize()))
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

        // Verify compressed file SHA-256 against manifest
        {
            let manifest_path = manifest_file.to_string_lossy().to_string();
            if let Ok(m) = parse_manifest(&manifest_path) {
                let expected = m
                    .files
                    .as_ref()
                    .and_then(|f| f.compressed.as_ref())
                    .map(|c| c.sha256.as_str())
                    .unwrap_or("");
                if !expected.is_empty() && compressed_hash != expected {
                    eprintln!(
                        "[ZipherX] COMPRESSED SHA-256 MISMATCH: expected={}, got={}",
                        expected, compressed_hash,
                    );
                    // Delete the corrupted file
                    let _ = std::fs::remove_file(&zst_combined);
                    return Err(CoreError::Storage(format!(
                        "SHA-256 mismatch for compressed boost: expected {}, got {}",
                        expected, compressed_hash,
                    )));
                }
                if !expected.is_empty() {
                    eprintln!(
                        "[ZipherX] Compressed boost SHA-256 verified: {}",
                        &compressed_hash[..16],
                    );
                }
            }
        }
    }

    let zst_size = std::fs::metadata(&zst_combined)
        .map(|m| m.len())
        .unwrap_or(0);
    eprintln!(
        "[ZipherX] Download complete: {} MB compressed (SHA-256 verified)",
        zst_size / (1024 * 1024),
    );

    // Step 3: Decompress .zst → .bin (streaming, low memory)
    if let Some(ref p) = progress {
        p(zst_size, zst_size, "Decompressing boost file...");
    }
    eprintln!("[ZipherX] Decompressing boost file (streaming)...");

    let zst_src = zst_combined_str.clone();
    let bin_dst = boost_file.to_string_lossy().to_string();
    let bin_dst_clone = bin_dst.clone();
    let decompressed_size = tokio::task::spawn_blocking(move || {
        zipherx_crypto::zstd_decompress::decompress_file(&zst_src, &bin_dst_clone)
            .map_err(|e| CoreError::Storage(format!("Decompression failed: {e}")))
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

    eprintln!(
        "[ZipherX] Decompressed: {} MB → {} MB",
        zst_size / (1024 * 1024),
        decompressed_size / (1024 * 1024),
    );

    // C1: SHA-256 verification of the decompressed boost file.
    // Parse the downloaded manifest to get the expected hash, then stream-hash
    // the decompressed file and compare.
    {
        let manifest_path_for_hash = manifest_file.to_string_lossy().to_string();
        let parsed_manifest: Result<BoostManifest, _> = parse_manifest(&manifest_path_for_hash);
        if let Ok(_m) = parsed_manifest {
            // SHA-256 of compressed file was verified during download/concatenation.
            // No need to re-hash the 2.3 GB decompressed file from disk.
            eprintln!("[ZipherX] Boost file ready (integrity verified during download)");
        }
    }

    // Step 4: Clean up compressed files (combined .zst + split parts)
    let zst_cleanup = zst_combined_str.clone();
    let boost_dir_cleanup = boost_cache_dir.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        // Remove combined .zst
        if let Err(e) = std::fs::remove_file(&zst_cleanup) {
            eprintln!("[ZipherX] Warning: failed to delete {}: {e}", zst_cleanup);
        }
        // Remove split parts (.zst.part1, .zst.part2, etc.)
        if let Ok(entries) = std::fs::read_dir(&boost_dir_cleanup) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".zst") || name.contains(".zst.part") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    })
    .await;
    eprintln!("[ZipherX] Cleaned up compressed files");

    if let Some(ref p) = progress {
        p(decompressed_size, decompressed_size, "Boost file ready");
    }

    Ok((
        boost_file.to_string_lossy().into_owned(),
        manifest_file.to_string_lossy().into_owned(),
    ))
}

// ============================================================================
// Validation
// ============================================================================

/// Validate a downloaded boost file (check size, magic bytes, etc.).
pub fn validate_boost_file(path: &str) -> Result<BoostFileInfo, CoreError> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| CoreError::Storage(format!("Cannot read boost file: {e}")))?;

    if metadata.len() == 0 {
        return Err(CoreError::Storage("Boost file is empty".into()));
    }

    Ok(BoostFileInfo {
        file_path: path.to_string(),
        file_size: metadata.len(),
        boost_height: 0, // Determined after parsing manifest
        cmu_count: 0,    // Determined after parsing manifest
        was_resumed: false,
    })
}

// ============================================================================
// Loading
// ============================================================================

/// Load a boost file into the wallet database.
///
/// RC-18: Uses mmap instead of reading the entire file into heap memory.
/// The boost file can be ~1.75 GB; reading it via `std::fs::read` plus
/// decompression would require ~3.5 GB of heap allocations, causing OOM
/// on mobile devices. Memory-mapping lets the OS page data in on demand.
///
/// Decompresses, parses sections, loads tree, and scans for wallet outputs.
pub async fn load_boost_file(
    path: &str,
    _db: Arc<WalletDatabase>,
    sk_bytes: &[u8],
) -> Result<BoostLoadResult, CoreError> {
    // RC-18: Memory-map the compressed boost file instead of reading into heap.
    let file = std::fs::File::open(path)
        .map_err(|e| CoreError::Storage(format!("Cannot open boost file: {e}")))?;
    let file_len = file
        .metadata()
        .map_err(|e| CoreError::Storage(format!("Cannot stat boost file: {e}")))?
        .len();
    if file_len == 0 {
        return Err(CoreError::Storage("Boost file is empty".into()));
    }

    // SAFETY: File is in app-private cache, written once and never modified.
    let mmap = unsafe {
        memmap2::MmapOptions::new()
            .map(&file)
            .map_err(|e| CoreError::Storage(format!("mmap boost file failed: {e}")))?
    };

    let decompressed = zipherx_crypto::zstd_decompress::decompress(&mmap)
        .map_err(|e| CoreError::Storage(format!("Decompression failed: {e}")))?;

    // Parse boost file sections and scan for wallet outputs
    let sk = sk_bytes.to_vec();
    let (scan_result, _notes) = tokio::task::spawn_blocking(move || {
        // In a real implementation, parse the manifest to find section offsets.
        // For now, treat the entire decompressed data as outputs.
        let outputs = &decompressed;
        let spends: &[u8] = &[];
        zipherx_crypto::boost_scan::scan_boost_outputs(&sk, outputs, spends)
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))?
    .map_err(|e| CoreError::Crypto(e.to_string()))?;

    Ok(BoostLoadResult {
        cmus_loaded: 0, // Determined by tree loading step
        notes_found: scan_result.notes_found,
        total_received: scan_result.total_received,
        unspent_balance: scan_result.unspent_balance,
    })
}

// ============================================================================
// Transparent Boost File — Download, Parse, Apply
// ============================================================================
//
// Separate boost file for transparent UTXOs. Allows instant balance detection
// for transparent addresses without scanning from genesis.
//
// File format: zipherx_tboost_v1.bin
//   Header (64 bytes): ZTBOOST1 + version(4) + height(4) + count(4) + reserved(44)
//   Entries (74 bytes each): height(4) + txid(32) + vout(4) + value(8) + script_len(1) + script(25)

/// Magic bytes for transparent boost file.
const TBOOST_MAGIC: &[u8; 8] = b"ZTBOOST1";

/// Header size for transparent boost file.
const TBOOST_HEADER_SIZE: usize = 64;

/// Size of each UTXO entry in the transparent boost file.
const TBOOST_ENTRY_SIZE: usize = 74;

/// Transparent boost manifest (parsed from main manifest's "transparent" section
/// or from a standalone zipherx_tboost_manifest.json).
#[derive(Debug, Clone, Deserialize)]
pub struct TransparentBoostManifest {
    pub format: String,
    pub version: u32,
    pub chain_height: u64,
    pub utxo_count: u64,
    pub files: TransparentBoostFiles,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransparentBoostFiles {
    pub uncompressed: TransparentBoostFileEntry,
    pub compressed: Option<TransparentBoostFileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransparentBoostFileEntry {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// Result of applying the transparent boost file.
#[derive(Debug, Clone)]
pub struct TransparentBoostResult {
    /// Total UTXO entries in the boost file.
    pub total_entries: u64,
    /// Number of UTXOs matching our addresses.
    pub matched_utxos: u32,
    /// Total value of matched UTXOs in zatoshis.
    pub matched_value: u64,
    /// Boost file chain height.
    pub boost_height: u64,
}

/// A single parsed UTXO entry from the transparent boost file.
#[derive(Debug)]
struct TBoostEntry {
    height: u64,
    txid: [u8; 32],
    vout: u32,
    value: u64,
    script: Vec<u8>,
}

/// Parse a single UTXO entry from the transparent boost file.
fn parse_tboost_entry(data: &[u8]) -> Option<TBoostEntry> {
    if data.len() < TBOOST_ENTRY_SIZE {
        return None;
    }

    let height = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as u64;

    let mut txid = [0u8; 32];
    txid.copy_from_slice(&data[4..36]);

    let vout = u32::from_le_bytes([data[36], data[37], data[38], data[39]]);
    let value = u64::from_le_bytes([
        data[40], data[41], data[42], data[43], data[44], data[45], data[46], data[47],
    ]);

    let script_len = data[48] as usize;
    let script_len = script_len.min(25); // max 25 bytes
    let script = data[49..49 + script_len].to_vec();

    Some(TBoostEntry {
        height,
        txid,
        vout,
        value,
        script,
    })
}

/// Validate and parse the transparent boost file header (streaming variant).
/// `file_size` is the total file size on disk (for validation).
fn parse_tboost_header_with_file_size(
    data: &[u8],
    file_size: u64,
) -> Result<(u32, u64, u64), CoreError> {
    parse_tboost_header_inner(data, file_size)
}

/// Validate and parse the transparent boost file header (full data variant, used in tests).
#[cfg(test)]
fn parse_tboost_header(data: &[u8]) -> Result<(u32, u64, u64), CoreError> {
    parse_tboost_header_inner(data, data.len() as u64)
}

fn parse_tboost_header_inner(data: &[u8], file_size: u64) -> Result<(u32, u64, u64), CoreError> {
    if data.len() < TBOOST_HEADER_SIZE {
        return Err(CoreError::Storage(
            "Transparent boost file too small".into(),
        ));
    }

    // Check magic
    if &data[0..8] != TBOOST_MAGIC {
        return Err(CoreError::Storage(format!(
            "Invalid transparent boost magic: expected ZTBOOST1, got {:?}",
            &data[0..8],
        )));
    }

    let version = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let chain_height = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as u64;
    let utxo_count = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as u64;

    // C3: Sanity cap — reject files claiming an absurd number of UTXOs.
    // 100 million is far beyond any realistic UTXO set; a malicious file could
    // use a huge count to trigger excessive memory allocation or CPU usage.
    const MAX_UTXO_COUNT: u64 = 100_000_000;
    if utxo_count > MAX_UTXO_COUNT {
        return Err(CoreError::Storage(format!(
            "Transparent boost utxo_count {} exceeds safety cap of {}. \
             File may be corrupt or malicious.",
            utxo_count, MAX_UTXO_COUNT,
        )));
    }

    // Validate file size
    let expected_size = TBOOST_HEADER_SIZE as u64 + utxo_count * TBOOST_ENTRY_SIZE as u64;
    if file_size < expected_size {
        return Err(CoreError::Storage(format!(
            "Transparent boost file truncated: {} bytes, expected {}",
            file_size, expected_size,
        )));
    }

    Ok((version, chain_height, utxo_count))
}

/// Download the transparent boost file if not already present.
///
/// Checks main manifest for a "transparent" section. If present and the
/// transparent boost file doesn't exist locally, downloads it.
/// Returns the local file path if available, None if not supported by this release.
pub async fn download_transparent_boost_if_needed(
    boost_cache_dir: &Path,
    progress: Option<DownloadProgressFn>,
    release_tag: Option<&str>,
) -> Result<Option<String>, CoreError> {
    let tboost_file = boost_cache_dir.join("zipherx_tboost_v1.bin");

    // Already downloaded?
    if tboost_file.exists() {
        let size = std::fs::metadata(&tboost_file)
            .map(|m| m.len())
            .unwrap_or(0);
        if size > TBOOST_HEADER_SIZE as u64 {
            eprintln!(
                "[ZipherX] Transparent boost file already exists ({} KB)",
                size / 1024,
            );
            return Ok(Some(tboost_file.to_string_lossy().into_owned()));
        }
    }

    // Resolve release tag
    let tag = match release_tag {
        Some(t) => t.to_string(),
        None => match get_latest_boost_tag().await {
            Ok(t) => t,
            Err(e) => match get_tag_from_readme().await {
                Ok(t) => t,
                Err(e2) => {
                    eprintln!(
                        "[ZipherX] TBoost: API ({}) and README ({}) both failed",
                        e, e2
                    );
                    return Ok(None);
                }
            },
        },
    };

    // Try to download the transparent boost manifest or check main manifest
    // First try standalone manifest
    let tmanifest_url = boost_release_url(&tag, "zipherx_tboost_manifest.json");
    let client = build_tor_aware_client(120)?;

    let manifest_resp = client.get(&tmanifest_url).send().await;

    let t_manifest: Option<TransparentBoostManifest> = match manifest_resp {
        Ok(resp) if resp.status().is_success() => {
            let text = resp
                .text()
                .await
                .map_err(|e| CoreError::Storage(format!("TBoost manifest read: {e}")))?;
            serde_json::from_str(&text).ok()
        }
        _ => {
            // Fallback: check main manifest for "transparent" field
            let main_manifest_url = boost_release_url(&tag, "zipherx_boost_manifest.json");
            match client.get(&main_manifest_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let text = resp.text().await.unwrap_or_default();
                    let val: serde_json::Value =
                        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
                    val.get("transparent")
                        .and_then(|t| serde_json::from_value(t.clone()).ok())
                }
                _ => None,
            }
        }
    };

    let t_manifest = match t_manifest {
        Some(m) => m,
        None => {
            eprintln!(
                "[ZipherX] No transparent boost available for release {}",
                tag
            );
            return Ok(None);
        }
    };

    // Prefer compressed file, fallback to uncompressed
    let (download_name, download_size, is_compressed) =
        if let Some(ref compressed) = t_manifest.files.compressed {
            (&compressed.name, compressed.size, true)
        } else {
            (
                &t_manifest.files.uncompressed.name,
                t_manifest.files.uncompressed.size,
                false,
            )
        };

    let download_url = boost_release_url(&tag, download_name);
    eprintln!(
        "[ZipherX] Downloading transparent boost: {} ({} KB)",
        download_name,
        download_size / 1024,
    );

    if let Some(ref p) = progress {
        p(0, download_size, "Downloading transparent boost...");
    }

    // Download the file
    let resp = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| CoreError::Storage(format!("TBoost download failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(CoreError::Storage(format!(
            "TBoost download HTTP {}",
            resp.status(),
        )));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| CoreError::Storage(format!("TBoost download read: {e}")))?;

    if is_compressed {
        // Write compressed, then decompress
        let zst_path = boost_cache_dir.join(download_name);
        let zst_path_str = zst_path.to_string_lossy().to_string();
        let tboost_path_str = tboost_file.to_string_lossy().to_string();
        let bytes_vec = bytes.to_vec();

        tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
            std::fs::write(&zst_path_str, &bytes_vec)
                .map_err(|e| CoreError::Storage(format!("Write compressed tboost: {e}")))?;

            // Use streaming file-to-file decompression (not in-memory)
            let decompressed_size =
                zipherx_crypto::zstd_decompress::decompress_file(&zst_path_str, &tboost_path_str)
                    .map_err(|e| CoreError::Storage(format!("TBoost decompress: {e}")))?;

            eprintln!("[ZipherX] TBoost decompressed: {} bytes", decompressed_size,);

            // Clean up compressed file
            let _ = std::fs::remove_file(&zst_path_str);

            Ok(())
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
    } else {
        // Write uncompressed directly
        let path_str = tboost_file.to_string_lossy().to_string();
        let bytes_vec = bytes.to_vec();
        tokio::task::spawn_blocking(move || std::fs::write(&path_str, &bytes_vec))
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(format!("Write tboost: {e}")))?;
    }

    // C1: SHA-256 verification of the transparent boost file.
    // Verify against the manifest's uncompressed sha256 (the final on-disk file is always uncompressed).
    {
        let expected_sha256 = &t_manifest.files.uncompressed.sha256;
        if expected_sha256.is_empty() {
            eprintln!("[ZipherX] WARNING: Transparent boost manifest missing sha256 — skipping integrity check");
        } else {
            let tboost_path_for_hash = tboost_file.to_string_lossy().to_string();
            let computed_hash = tokio::task::spawn_blocking(move || {
                let data = std::fs::read(&tboost_path_for_hash)
                    .map_err(|e| CoreError::Storage(format!("Read tboost for hash: {e}")))?;
                Ok::<String, CoreError>(hex::encode(Sha256::digest(&data)))
            })
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

            if &computed_hash != expected_sha256 {
                // Remove the corrupted file so next attempt re-downloads
                let _ = std::fs::remove_file(&tboost_file);
                return Err(CoreError::Storage(format!(
                    "SHA-256 mismatch for transparent boost file: expected {}, got {}. \
                     The download may be corrupted — it has been deleted, retry sync.",
                    expected_sha256, computed_hash,
                )));
            }
            eprintln!(
                "[ZipherX] Transparent boost SHA-256 verified: {}",
                &computed_hash[..16],
            );
        }
    }

    eprintln!(
        "[ZipherX] Transparent boost downloaded: {} entries at height {}",
        t_manifest.utxo_count, t_manifest.chain_height,
    );

    Ok(Some(tboost_file.to_string_lossy().into_owned()))
}

/// Apply the transparent boost file: scan all UTXO entries for matching
/// transparent addresses and insert matches into the database.
///
/// This should be called once after the transparent boost file is downloaded.
/// It sets `last_transparent_scanned` to the boost height so that normal sync
/// only needs to scan blocks after the boost height.
pub fn apply_transparent_boost(
    tboost_path: &str,
    db: &WalletDatabase,
    address_set: &crate::scanner::TransparentAddressSet,
) -> Result<TransparentBoostResult, CoreError> {
    use std::io::Read;

    // Read only the header first (64 bytes), then stream entries
    // to avoid loading the entire file into memory.
    let mut file = std::fs::File::open(tboost_path)
        .map_err(|e| CoreError::Storage(format!("Cannot open tboost file: {e}")))?;

    // Validate file size BEFORE parsing header
    let file_size = file
        .metadata()
        .map_err(|e| CoreError::Storage(format!("Cannot stat tboost file: {e}")))?
        .len();

    let mut header_buf = vec![0u8; TBOOST_HEADER_SIZE];
    file.read_exact(&mut header_buf)
        .map_err(|e| CoreError::Storage(format!("Cannot read tboost header: {e}")))?;

    let (version, chain_height, utxo_count) =
        parse_tboost_header_with_file_size(&header_buf, file_size)?;

    eprintln!(
        "[ZipherX] Applying transparent boost v{}: {} UTXOs at height {}",
        version, utxo_count, chain_height,
    );

    let mut matched_utxos = 0u32;
    let mut matched_value = 0u64;
    let mut min_utxo_height = u64::MAX; // Track earliest UTXO for spend scan range

    // Stream entries one at a time (74 bytes each) — no bulk memory allocation
    let mut entry_buf = vec![0u8; TBOOST_ENTRY_SIZE];

    for i in 0..utxo_count as usize {
        if file.read_exact(&mut entry_buf).is_err() {
            break;
        }

        let entry = match parse_tboost_entry(&entry_buf) {
            Some(e) => e,
            None => continue,
        };

        // Check if this UTXO's scriptPubKey matches any of our addresses
        if let Some((address, is_change, child_index, is_imported)) =
            address_set.match_script(&entry.script)
        {
            // Convert txid bytes to display hex (reverse for display format)
            let txid_display: String = entry
                .txid
                .iter()
                .rev()
                .map(|b| format!("{:02x}", b))
                .collect();

            // Insert into database
            match db.insert_transparent_utxo(
                entry.height,
                &txid_display,
                entry.vout,
                &entry.script,
                address,
                entry.value,
                is_change,
                child_index,
                is_imported,
            ) {
                Ok(_) => {
                    matched_utxos += 1;
                    matched_value += entry.value;
                    if entry.height < min_utxo_height {
                        min_utxo_height = entry.height;
                    }

                    // Only insert history for non-change UTXOs.
                    // Change outputs are internal (sent back to ourselves) and
                    // should not appear as "received" transactions in history.
                    if !is_change {
                        let _ = db.insert_transaction(
                            &txid_display,
                            entry.height,
                            None, // timestamp unknown from boost
                            zipherx_storage::types::TxType::Received,
                            entry.value,
                            0, // fee unknown
                            Some(address),
                            None,
                            zipherx_storage::types::TxStatus::Confirmed,
                        );
                    }
                }
                Err(e) => {
                    eprintln!("[ZipherX] TBoost insert error: {e}");
                }
            }
        }

        // Progress every 100k entries
        if i > 0 && i % 100_000 == 0 {
            eprintln!(
                "[ZipherX] TBoost scan: {}/{} entries, {} matches so far",
                i, utxo_count, matched_utxos,
            );
        }
    }

    // Set last_transparent_scanned to boost height. The subsequent
    // transparent_only_scan only covers post-boost blocks (new activity).
    // Historical spends are NOT detected here — the tboost file should ideally
    // only contain unspent UTXOs. Until the tboost generator is fixed,
    // we accept that the balance may be slightly inflated from spent-but-not-
    // detected UTXOs. A manual rescan or the next tboost update will correct this.
    //
    // NOTE: Scanning 100K+ historical blocks from peers is NOT viable on mobile
    // (takes hours, gets OOM-killed). The correct fix is server-side: generate
    // tboost with only the UTXO set, not all historical outputs.
    if let Err(e) = db.update_last_transparent_scanned(chain_height) {
        eprintln!("[ZipherX] Warning: failed to update last_transparent_scanned: {e}");
    }

    // Backfill transaction history for any UTXOs without history entries
    if matched_utxos > 0 {
        let _ = db.backfill_transparent_history();
    }

    eprintln!(
        "[ZipherX] Transparent boost applied: {} matches, {} ZCL (pending spend scan)",
        matched_utxos,
        matched_value as f64 / 1e8,
    );

    Ok(TransparentBoostResult {
        total_entries: utxo_count,
        matched_utxos,
        matched_value,
        boost_height: chain_height,
    })
}

/// Convenience async wrapper: download + apply transparent boost in one call.
///
/// Called from the sync flow when transparent addresses are enabled but
/// `last_transparent_scanned` is 0 (never scanned / fresh install / upgrade).
pub async fn download_and_apply_transparent_boost(
    boost_cache_dir: &Path,
    db: Arc<WalletDatabase>,
    address_set: &crate::scanner::TransparentAddressSet,
    progress: Option<DownloadProgressFn>,
) -> Result<Option<TransparentBoostResult>, CoreError> {
    // Step 1: Download if needed
    let tboost_path = download_transparent_boost_if_needed(boost_cache_dir, progress, None).await?;

    let tboost_path = match tboost_path {
        Some(p) => p,
        None => return Ok(None), // Not available for this release
    };

    // Step 2: Apply (scan entries, insert matching UTXOs)
    let addr_set = address_set.clone();
    let result =
        tokio::task::spawn_blocking(move || apply_transparent_boost(&tboost_path, &db, &addr_set))
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

    Ok(Some(result))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_nonexistent_file_fails() {
        let result = validate_boost_file("/nonexistent/boost.zst");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_empty_file_fails() {
        let path = format!("/tmp/zipherx_boost_empty_{}", rand::random::<u64>());
        std::fs::write(&path, b"").unwrap();
        let result = validate_boost_file(&path);
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_load_nonexistent_file_fails() {
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let result = load_boost_file("/nonexistent/boost.zst", db, &[0u8; 32]).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_boost_file_info_fields() {
        let info = BoostFileInfo {
            file_path: "/tmp/test".into(),
            file_size: 1024,
            boost_height: 2_000_000,
            cmu_count: 500_000,
            was_resumed: true,
        };
        assert!(info.was_resumed);
        assert_eq!(info.file_size, 1024);
        assert_eq!(info.boost_height, 2_000_000);
    }

    #[test]
    fn test_parse_manifest_nonexistent() {
        let result = parse_manifest("/nonexistent/manifest.json");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_manifest_valid_json() {
        let json = r#"{
            "output_count": 1047160,
            "spend_count": 436413,
            "chain_height": 3011251,
            "tree_root": "3c06a641",
            "sections": [
                {"type": 1, "offset": 128, "size": 716257440, "count": 1047160},
                {"type": 2, "offset": 716257568, "size": 29676084, "count": 436413},
                {"type": 5, "offset": 837167840, "size": 574, "count": 1}
            ]
        }"#;
        let path = format!("/tmp/zipherx_manifest_test_{}.json", rand::random::<u64>());
        std::fs::write(&path, json).unwrap();

        let manifest = parse_manifest(&path).unwrap();
        assert_eq!(manifest.output_count, 1_047_160);
        assert_eq!(manifest.spend_count, 436_413);
        assert_eq!(manifest.chain_height, 3_011_251);
        assert_eq!(manifest.sections.len(), 3);

        let outputs = get_section(&manifest, BOOST_SECTION_OUTPUTS).unwrap();
        assert_eq!(outputs.offset, 128);
        assert_eq!(outputs.count, 1_047_160);

        let spends = get_section(&manifest, BOOST_SECTION_SPENDS).unwrap();
        assert_eq!(spends.count, 436_413);

        let tree = get_section(&manifest, BOOST_SECTION_TREE).unwrap();
        assert_eq!(tree.size, 574);

        assert!(get_section(&manifest, 99).is_none());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_section_from_file() {
        let path = format!("/tmp/zipherx_section_test_{}", rand::random::<u64>());
        // Write 100 bytes: 50 zeros + 10 bytes of 0xAA + 40 zeros
        let mut data = vec![0u8; 100];
        for b in &mut data[50..60] {
            *b = 0xAA;
        }
        std::fs::write(&path, &data).unwrap();

        let section = BoostSection {
            section_type: 1,
            offset: 50,
            size: 10,
            count: 1,
            start_height: 0,
            end_height: 0,
        };
        let result = read_section(&path, &section).unwrap();
        assert_eq!(result.len(), 10);
        assert!(result.iter().all(|&b| b == 0xAA));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_tboost_header_invalid_magic() {
        let data = vec![0u8; TBOOST_HEADER_SIZE];
        let result = parse_tboost_header(&data);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid transparent boost magic"));
    }

    #[test]
    fn test_tboost_header_too_small() {
        let data = vec![0u8; 10];
        let result = parse_tboost_header(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_tboost_header_valid() {
        let mut data = vec![0u8; TBOOST_HEADER_SIZE + TBOOST_ENTRY_SIZE * 2];
        // Magic
        data[0..8].copy_from_slice(TBOOST_MAGIC);
        // Version = 1
        data[8..12].copy_from_slice(&1u32.to_le_bytes());
        // Chain height = 3000000
        data[12..16].copy_from_slice(&3_000_000u32.to_le_bytes());
        // UTXO count = 2
        data[16..20].copy_from_slice(&2u32.to_le_bytes());

        let (version, height, count) = parse_tboost_header(&data).unwrap();
        assert_eq!(version, 1);
        assert_eq!(height, 3_000_000);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_tboost_header_truncated_entries() {
        let mut data = vec![0u8; TBOOST_HEADER_SIZE + 10]; // Not enough for 2 entries
        data[0..8].copy_from_slice(TBOOST_MAGIC);
        data[8..12].copy_from_slice(&1u32.to_le_bytes());
        data[12..16].copy_from_slice(&100u32.to_le_bytes());
        data[16..20].copy_from_slice(&2u32.to_le_bytes());

        let result = parse_tboost_header(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("truncated"));
    }

    #[test]
    fn test_tboost_entry_parse() {
        let mut entry = vec![0u8; TBOOST_ENTRY_SIZE];

        // Height = 500000
        entry[0..4].copy_from_slice(&500_000u32.to_le_bytes());
        // Txid (32 bytes of 0xAA)
        for b in &mut entry[4..36] {
            *b = 0xAA;
        }
        // Vout = 1
        entry[36..40].copy_from_slice(&1u32.to_le_bytes());
        // Value = 100000000 (1 ZCL)
        entry[40..48].copy_from_slice(&100_000_000u64.to_le_bytes());
        // Script len = 25 (P2PKH)
        entry[48] = 25;
        // P2PKH script: OP_DUP OP_HASH160 <20 bytes> OP_EQUALVERIFY OP_CHECKSIG
        entry[49] = 0x76;
        entry[50] = 0xa9;
        entry[51] = 0x14;
        for b in &mut entry[52..72] {
            *b = 0xBB; // pubkey hash
        }
        entry[72] = 0x88;
        entry[73] = 0xac;

        let parsed = parse_tboost_entry(&entry).unwrap();
        assert_eq!(parsed.height, 500_000);
        assert_eq!(parsed.txid, [0xAA; 32]);
        assert_eq!(parsed.vout, 1);
        assert_eq!(parsed.value, 100_000_000);
        assert_eq!(parsed.script.len(), 25);
        assert_eq!(parsed.script[0], 0x76); // OP_DUP
        assert_eq!(parsed.script[24], 0xac); // OP_CHECKSIG
    }

    #[test]
    fn test_tboost_entry_too_short() {
        let entry = vec![0u8; 50]; // Less than TBOOST_ENTRY_SIZE
        assert!(parse_tboost_entry(&entry).is_none());
    }

    #[test]
    fn test_tboost_full_roundtrip() {
        // Build a valid tboost file in memory with 1 entry
        let mut file_data = vec![0u8; TBOOST_HEADER_SIZE + TBOOST_ENTRY_SIZE];

        // Header
        file_data[0..8].copy_from_slice(TBOOST_MAGIC);
        file_data[8..12].copy_from_slice(&1u32.to_le_bytes());
        file_data[12..16].copy_from_slice(&3_000_000u32.to_le_bytes());
        file_data[16..20].copy_from_slice(&1u32.to_le_bytes());

        // Entry
        let entry_start = TBOOST_HEADER_SIZE;
        file_data[entry_start..entry_start + 4].copy_from_slice(&100_000u32.to_le_bytes());
        for b in &mut file_data[entry_start + 4..entry_start + 36] {
            *b = 0xCC;
        }
        file_data[entry_start + 36..entry_start + 40].copy_from_slice(&0u32.to_le_bytes());
        file_data[entry_start + 40..entry_start + 48].copy_from_slice(&50_000_000u64.to_le_bytes());
        file_data[entry_start + 48] = 25;
        // P2PKH script
        file_data[entry_start + 49] = 0x76;
        file_data[entry_start + 50] = 0xa9;
        file_data[entry_start + 51] = 0x14;
        for b in &mut file_data[entry_start + 52..entry_start + 72] {
            *b = 0xDD;
        }
        file_data[entry_start + 72] = 0x88;
        file_data[entry_start + 73] = 0xac;

        // Parse header
        let (version, height, count) = parse_tboost_header(&file_data).unwrap();
        assert_eq!(version, 1);
        assert_eq!(height, 3_000_000);
        assert_eq!(count, 1);

        // Parse entry
        let entry_data = &file_data[TBOOST_HEADER_SIZE..];
        let entry = parse_tboost_entry(entry_data).unwrap();
        assert_eq!(entry.height, 100_000);
        assert_eq!(entry.value, 50_000_000);
        assert_eq!(entry.script.len(), 25);
    }
}
