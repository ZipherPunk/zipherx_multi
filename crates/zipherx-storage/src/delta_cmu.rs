//! DeltaCMUStore — binary file management for post-boost blockchain data.
//!
//! Manages delta shielded outputs, nullifiers, sapling roots, and manifest
//! as binary files on disk. Format matches the Swift implementation for
//! cross-platform compatibility.
//!
//! STOR-003: TODO — Delta CMU files are not encrypted at rest. These binary
//! files contain blockchain metadata (note commitments, nullifiers, sapling
//! roots) in plaintext. Consider encrypting with a key derived from the
//! wallet's encryption key to prevent data leakage on compromised devices.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::types::StorageError;

// ---------------------------------------------------------------
// Binary record sizes (must match Swift exactly)
// ---------------------------------------------------------------

/// Output record v2: height(4) + index(4) + cmu(32) + epk(32) + ciphertext(580) + txid(32) = 684 bytes.
pub const OUTPUT_RECORD_SIZE: usize = 684;

/// Legacy v1 record size (no txid field). Used for backward-compatible reads.
pub const OUTPUT_RECORD_SIZE_V1: usize = 652;

/// Nullifier record: height(4) + txid(32) + nullifier(32) = 68 bytes.
pub const NULLIFIER_RECORD_SIZE: usize = 68;

/// Sapling root entry: height(4) + root(32) + padding(4) = 40 bytes.
pub const SAPLING_ROOT_ENTRY_SIZE: usize = 40;

// ---------------------------------------------------------------
// Data types
// ---------------------------------------------------------------

/// A shielded output record from the delta bundle.
#[derive(Debug, Clone)]
pub struct DeltaOutput {
    /// Block height.
    pub height: u32,
    /// Output index within the block.
    pub index: u32,
    /// Note commitment (32 bytes).
    pub cmu: Vec<u8>,
    /// Ephemeral public key (32 bytes).
    pub epk: Vec<u8>,
    /// Encrypted ciphertext (580 bytes).
    pub ciphertext: Vec<u8>,
    /// Transaction ID (32 bytes, raw wire order). Added in v2.
    pub txid: Vec<u8>,
}

/// A nullifier record from the delta bundle.
#[derive(Debug, Clone)]
pub struct DeltaNullifier {
    /// Block height.
    pub height: u32,
    /// Transaction ID (32 bytes, raw).
    pub txid: Vec<u8>,
    /// Nullifier (32 bytes).
    pub nullifier: Vec<u8>,
}

/// Delta bundle manifest (JSON serialized).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaBundleManifest {
    /// Start height of delta range.
    pub start_height: u64,
    /// End height of delta range.
    pub end_height: u64,
    /// Number of output records.
    pub output_count: u64,
    /// Number of CMUs (may differ from output_count after dedup).
    pub cmu_count: u64,
    /// Tree root at end_height (hex string).
    pub tree_root: Option<String>,
    /// Last updated timestamp.
    pub updated_at: u64,
}

// ---------------------------------------------------------------
// DeltaCMUStore
// ---------------------------------------------------------------

/// File-based storage for delta CMU bundle data.
pub struct DeltaCMUStore {
    base_dir: PathBuf,
}

impl DeltaCMUStore {
    /// Create a new delta CMU store at the given base directory.
    pub fn new(base_dir: &Path) -> Result<Self, StorageError> {
        fs::create_dir_all(base_dir)?;
        Ok(Self {
            base_dir: base_dir.to_path_buf(),
        })
    }

    /// Get the base directory path.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    fn outputs_path(&self) -> PathBuf {
        self.base_dir.join("shielded_outputs_delta.bin")
    }

    fn manifest_path(&self) -> PathBuf {
        self.base_dir.join("delta_manifest.json")
    }

    fn sapling_roots_path(&self) -> PathBuf {
        self.base_dir.join("delta_sapling_roots.bin")
    }

    fn nullifiers_path(&self) -> PathBuf {
        self.base_dir.join("delta_nullifiers.bin")
    }

    // ---------------------------------------------------------------
    // Manifest
    // ---------------------------------------------------------------

    /// Check if a delta bundle exists (manifest file present).
    pub fn has_delta_bundle(&self) -> bool {
        self.manifest_path().exists()
    }

    /// Get the delta bundle manifest.
    pub fn get_manifest(&self) -> Result<Option<DeltaBundleManifest>, StorageError> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&path)?;
        let manifest: DeltaBundleManifest = serde_json::from_str(&data)?;
        Ok(Some(manifest))
    }

    /// Get the delta bundle end height, or 0 if no bundle.
    pub fn get_delta_end_height(&self) -> Result<u64, StorageError> {
        match self.get_manifest()? {
            Some(m) => Ok(m.end_height),
            None => Ok(0),
        }
    }

    /// Save the manifest.
    ///
    /// RS-4: fsync the manifest file to ensure durability. The manifest is the
    /// critical metadata that tracks delta bundle state — a partial write here
    /// could corrupt the sync state on power loss.
    fn save_manifest(&self, manifest: &DeltaBundleManifest) -> Result<(), StorageError> {
        let json = serde_json::to_string_pretty(manifest)?;
        let path = self.manifest_path();
        let mut file = fs::File::create(&path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }

    /// Set the delta manifest end_height (creates manifest if needed).
    /// Used after boost scan to record that outputs up to this height are covered.
    pub fn set_end_height(&self, end_height: u64) -> Result<(), StorageError> {
        let mut manifest = self.get_manifest()?.unwrap_or(DeltaBundleManifest {
            start_height: 0,
            end_height,
            output_count: 0,
            cmu_count: 0,
            tree_root: None,
            updated_at: 0,
        });
        manifest.end_height = end_height;
        manifest.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.save_manifest(&manifest)
    }

    /// Update the delta manifest with validated end_height and tree_root.
    ///
    /// Called after the combined tree (boost + post-boost) root is validated
    /// against the blockchain's finalsaplingroot. This marks the delta store
    /// as part of the "effective boost" for future sessions.
    pub fn update_manifest_verified(
        &self,
        end_height: u64,
        tree_root: &str,
    ) -> Result<(), StorageError> {
        let mut manifest = self.get_manifest()?.unwrap_or(DeltaBundleManifest {
            start_height: 0,
            end_height,
            output_count: 0,
            cmu_count: 0,
            tree_root: None,
            updated_at: 0,
        });
        manifest.end_height = end_height;
        manifest.tree_root = Some(tree_root.to_string());
        manifest.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.save_manifest(&manifest)
    }

    // ---------------------------------------------------------------
    // Shielded outputs
    // ---------------------------------------------------------------

    /// Load all CMUs from the delta bundle, sorted by (height, index).
    pub fn load_cmus(&self) -> Result<Vec<(u32, Vec<u8>)>, StorageError> {
        let outputs = self.load_outputs()?;
        let mut sorted: Vec<&DeltaOutput> = outputs.iter().collect();
        sorted.sort_by_key(|o| (o.height, o.index));
        let cmus = sorted.iter().map(|o| (o.height, o.cmu.clone())).collect();
        Ok(cmus)
    }

    /// Load CMUs for a specific height range (inclusive).
    /// Sorted by (height, index) to match blockchain commitment order.
    pub fn load_cmus_for_range(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Vec<(u32, Vec<u8>)>, StorageError> {
        let outputs = self.load_outputs()?;
        let mut filtered: Vec<&DeltaOutput> = outputs
            .iter()
            .filter(|o| o.height as u64 >= start && o.height as u64 <= end)
            .collect();
        // Sort by (height, index) to ensure correct intra-block ordering
        filtered.sort_by_key(|o| (o.height, o.index));
        let cmus = filtered.iter().map(|o| (o.height, o.cmu.clone())).collect();
        Ok(cmus)
    }

    /// Load all output records from the binary file.
    /// Handles both v1 (652 bytes, no txid) and v2 (684 bytes, with txid) formats.
    pub fn load_outputs(&self) -> Result<Vec<DeltaOutput>, StorageError> {
        let path = self.outputs_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let data = fs::read(&path)?;
        let record_size = detect_record_size(data.len());
        if record_size == 0 {
            return Err(StorageError::QueryFailed(format!(
                "Delta outputs file corrupt: {} bytes not divisible by {} or {}",
                data.len(),
                OUTPUT_RECORD_SIZE,
                OUTPUT_RECORD_SIZE_V1,
            )));
        }

        let count = data.len() / record_size;
        let mut outputs = Vec::with_capacity(count);
        for i in 0..count {
            let offset = i * record_size;
            let record = &data[offset..offset + record_size];
            outputs.push(parse_output_record(record));
        }
        Ok(outputs)
    }

    /// Remove duplicate output records from the binary file.
    ///
    /// Deduplicates by (height, index), keeping the first occurrence.
    /// Rewrites the file in place. Returns the number of duplicates removed.
    pub fn dedup_outputs(&self) -> Result<usize, StorageError> {
        let all = self.load_outputs()?;
        if all.is_empty() {
            return Ok(0);
        }

        let mut seen = std::collections::HashSet::new();
        let mut unique: Vec<&DeltaOutput> = Vec::with_capacity(all.len());
        for o in &all {
            if seen.insert((o.height, o.index)) {
                unique.push(o);
            }
        }

        let removed = all.len() - unique.len();
        if removed == 0 {
            return Ok(0);
        }

        // Rewrite the file with only unique records
        let path = self.outputs_path();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;

        for output in &unique {
            let record = serialize_output_record(output);
            file.write_all(&record)?;
        }
        file.sync_all()?;

        // Update manifest count
        if let Some(mut manifest) = self.get_manifest()? {
            manifest.output_count = unique.len() as u64;
            manifest.cmu_count = manifest.output_count;
            manifest.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            self.save_manifest(&manifest)?;
        }

        Ok(removed)
    }

    /// Append output records with dedup by (height, index) (FIX #784).
    ///
    /// Updates the manifest with new height range and counts.
    pub fn append_outputs(
        &self,
        outputs: &[DeltaOutput],
        from_height: u64,
        to_height: u64,
        tree_root: Option<&str>,
    ) -> Result<usize, StorageError> {
        if outputs.is_empty() {
            return Ok(0);
        }

        // Load existing records to dedup
        let existing = self.load_outputs()?;
        let mut existing_keys: std::collections::HashSet<(u32, u32)> =
            existing.iter().map(|o| (o.height, o.index)).collect();

        let path = self.outputs_path();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        let mut appended = 0;
        for output in outputs {
            if existing_keys.insert((output.height, output.index)) {
                let record = serialize_output_record(output);
                file.write_all(&record)?;
                appended += 1;
            }
        }
        // STOR-004: Flush data to disk before updating manifest
        file.sync_all()?;

        // Update manifest
        let mut manifest = self.get_manifest()?.unwrap_or(DeltaBundleManifest {
            start_height: from_height,
            end_height: to_height,
            output_count: 0,
            cmu_count: 0,
            tree_root: None,
            updated_at: 0,
        });
        manifest.output_count = (existing.len() + appended) as u64;
        manifest.cmu_count = manifest.output_count;
        if to_height > manifest.end_height {
            manifest.end_height = to_height;
        }
        if let Some(root) = tree_root {
            manifest.tree_root = Some(root.to_string());
        }
        manifest.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.save_manifest(&manifest)?;

        Ok(appended)
    }

    /// Append output records WITHOUT dedup check.
    ///
    /// Use when the caller guarantees no duplicates (e.g., cursor-based
    /// block fetching where each block is processed exactly once).
    /// Avoids the full-file read that `append_outputs` does for dedup.
    pub fn append_outputs_no_dedup(
        &self,
        outputs: &[DeltaOutput],
        from_height: u64,
        to_height: u64,
        tree_root: Option<&str>,
    ) -> Result<usize, StorageError> {
        if outputs.is_empty() {
            return Ok(0);
        }

        let path = self.outputs_path();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        for output in outputs {
            let record = serialize_output_record(output);
            file.write_all(&record)?;
        }
        // STOR-004: Flush data to disk before updating manifest
        file.sync_all()?;

        // Update manifest using lightweight output_count (file size only)
        let total_count = self.output_count()?;
        let mut manifest = self.get_manifest()?.unwrap_or(DeltaBundleManifest {
            start_height: from_height,
            end_height: to_height,
            output_count: 0,
            cmu_count: 0,
            tree_root: None,
            updated_at: 0,
        });
        manifest.output_count = total_count as u64;
        manifest.cmu_count = manifest.output_count;
        if from_height < manifest.start_height || manifest.start_height == 0 {
            manifest.start_height = from_height;
        }
        if to_height > manifest.end_height {
            manifest.end_height = to_height;
        }
        if let Some(root) = tree_root {
            manifest.tree_root = Some(root.to_string());
        }
        manifest.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.save_manifest(&manifest)?;

        Ok(outputs.len())
    }

    /// Clear the delta bundle (FIX #1254).
    ///
    /// When `verified` is true and `force` is false, this REFUSES to clear
    /// (returns `StorageError::DeltaBundleVerified`).
    pub fn clear_delta_bundle(&self, force: bool, verified: bool) -> Result<(), StorageError> {
        if verified && !force {
            return Err(StorageError::DeltaBundleVerified);
        }

        // Remove all delta files
        for path in &[
            self.outputs_path(),
            self.manifest_path(),
            self.sapling_roots_path(),
            self.nullifiers_path(),
        ] {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------
    // Sapling roots (FIX #1253)
    // ---------------------------------------------------------------

    /// Append sapling root entries to the companion file.
    pub fn append_sapling_roots_batch(
        &self,
        entries: &[(u64, Vec<u8>)],
    ) -> Result<(), StorageError> {
        if entries.is_empty() {
            return Ok(());
        }

        let path = self.sapling_roots_path();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        for (height, root) in entries {
            let mut record = Vec::with_capacity(SAPLING_ROOT_ENTRY_SIZE);
            record.extend_from_slice(&(*height as u32).to_le_bytes());
            // Pad or truncate root to 32 bytes
            let root_bytes = if root.len() >= 32 {
                &root[..32]
            } else {
                root.as_slice()
            };
            record.extend_from_slice(root_bytes);
            if root.len() < 32 {
                record.extend(std::iter::repeat(0u8).take(32 - root.len()));
            }
            // 4 bytes padding
            record.extend_from_slice(&[0u8; 4]);
            file.write_all(&record)?;
        }
        // STOR-004: Flush data to disk to ensure durability
        file.sync_all()?;
        Ok(())
    }

    /// Load sapling roots from the companion file.
    /// Returns entries in both byte orders for FIX #1230 compatibility.
    pub fn load_sapling_roots(&self) -> Result<Vec<(u64, Vec<u8>)>, StorageError> {
        let path = self.sapling_roots_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let data = fs::read(&path)?;
        if data.len() % SAPLING_ROOT_ENTRY_SIZE != 0 {
            return Err(StorageError::QueryFailed(format!(
                "Sapling roots file corrupt: {} bytes not divisible by {}",
                data.len(),
                SAPLING_ROOT_ENTRY_SIZE
            )));
        }

        let count = data.len() / SAPLING_ROOT_ENTRY_SIZE;
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let offset = i * SAPLING_ROOT_ENTRY_SIZE;
            let record = &data[offset..offset + SAPLING_ROOT_ENTRY_SIZE];
            let height = u32::from_le_bytes([record[0], record[1], record[2], record[3]]) as u64;
            let root = record[4..36].to_vec();
            entries.push((height, root));
        }
        Ok(entries)
    }

    // ---------------------------------------------------------------
    // Nullifiers (FIX #1289 v3)
    // ---------------------------------------------------------------

    /// Append nullifier records.
    pub fn append_nullifiers(&self, nullifiers: &[DeltaNullifier]) -> Result<(), StorageError> {
        if nullifiers.is_empty() {
            return Ok(());
        }

        let path = self.nullifiers_path();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        for nf in nullifiers {
            let record = serialize_nullifier_record(nf);
            file.write_all(&record)?;
        }
        // STOR-004: Flush data to disk to ensure durability
        file.sync_all()?;
        Ok(())
    }

    /// Load all nullifier records.
    pub fn load_nullifiers(&self) -> Result<Vec<DeltaNullifier>, StorageError> {
        let path = self.nullifiers_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let data = fs::read(&path)?;
        if data.len() % NULLIFIER_RECORD_SIZE != 0 {
            return Err(StorageError::QueryFailed(format!(
                "Nullifiers file corrupt: {} bytes not divisible by {}",
                data.len(),
                NULLIFIER_RECORD_SIZE
            )));
        }

        let count = data.len() / NULLIFIER_RECORD_SIZE;
        let mut nullifiers = Vec::with_capacity(count);
        for i in 0..count {
            let offset = i * NULLIFIER_RECORD_SIZE;
            let record = &data[offset..offset + NULLIFIER_RECORD_SIZE];
            nullifiers.push(parse_nullifier_record(record));
        }
        Ok(nullifiers)
    }

    /// Load nullifiers filtered to a height range (inclusive), using paged file reads.
    ///
    /// Reads the file in pages to avoid loading all records into memory at once.
    /// Returns only nullifiers with `height >= min_height && height <= max_height`.
    pub fn load_nullifiers_for_height_range(
        &self,
        min_height: u64,
        max_height: u64,
    ) -> Result<Vec<DeltaNullifier>, StorageError> {
        let path = self.nullifiers_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let mut file = fs::File::open(&path)?;
        let file_size = file.metadata()?.len() as usize;
        if file_size % NULLIFIER_RECORD_SIZE != 0 {
            return Err(StorageError::QueryFailed(format!(
                "Nullifiers file corrupt: {} bytes not divisible by {}",
                file_size, NULLIFIER_RECORD_SIZE
            )));
        }

        let total_records = file_size / NULLIFIER_RECORD_SIZE;
        let mut result = Vec::new();
        const PAGE_RECORDS: usize = 10_000;
        let mut offset: usize = 0;

        while offset < total_records {
            let records_to_read = PAGE_RECORDS.min(total_records - offset);
            let bytes_to_read = records_to_read * NULLIFIER_RECORD_SIZE;
            file.seek(SeekFrom::Start((offset * NULLIFIER_RECORD_SIZE) as u64))?;
            let mut buf = vec![0u8; bytes_to_read];
            file.read_exact(&mut buf)?;

            for i in 0..records_to_read {
                let rec_offset = i * NULLIFIER_RECORD_SIZE;
                let record = &buf[rec_offset..rec_offset + NULLIFIER_RECORD_SIZE];
                let height =
                    u32::from_le_bytes([record[0], record[1], record[2], record[3]]) as u64;
                if height >= min_height && height <= max_height {
                    result.push(parse_nullifier_record(record));
                }
            }

            offset += records_to_read;
        }

        Ok(result)
    }

    /// Get the total nullifier count from the file size.
    pub fn nullifier_count(&self) -> Result<usize, StorageError> {
        let path = self.nullifiers_path();
        if !path.exists() {
            return Ok(0);
        }
        let size = fs::metadata(&path)?.len() as usize;
        if size % NULLIFIER_RECORD_SIZE != 0 {
            return Ok(0);
        }
        Ok(size / NULLIFIER_RECORD_SIZE)
    }

    /// Get the total output count from the outputs file.
    /// Handles both v1 (652) and v2 (684) record sizes.
    pub fn output_count(&self) -> Result<usize, StorageError> {
        let path = self.outputs_path();
        if !path.exists() {
            return Ok(0);
        }
        let size = fs::metadata(&path)?.len() as usize;
        let record_size = detect_record_size(size);
        if record_size == 0 {
            return Ok(0);
        }
        Ok(size / record_size)
    }

    /// Load a page of output records by record offset and limit.
    ///
    /// Uses file seeking to avoid loading the entire file into memory.
    /// Handles both v1 (652) and v2 (684) record sizes.
    pub fn load_outputs_paged(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<DeltaOutput>, StorageError> {
        let path = self.outputs_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let mut file = fs::File::open(&path)?;
        let file_size = file.metadata()?.len() as usize;
        let record_size = detect_record_size(file_size);
        if record_size == 0 {
            return Ok(Vec::new());
        }
        let total_records = file_size / record_size;

        if offset >= total_records {
            return Ok(Vec::new());
        }

        let records_to_read = limit.min(total_records - offset);
        let byte_offset = offset * record_size;
        let bytes_to_read = records_to_read * record_size;

        file.seek(SeekFrom::Start(byte_offset as u64))?;
        let mut buf = vec![0u8; bytes_to_read];
        file.read_exact(&mut buf)?;

        let mut outputs = Vec::with_capacity(records_to_read);
        for i in 0..records_to_read {
            let record_offset = i * record_size;
            let record = &buf[record_offset..record_offset + record_size];
            outputs.push(parse_output_record(record));
        }
        Ok(outputs)
    }

    /// Load CMUs from a height range, paged by record offset.
    ///
    /// Uses `load_outputs_paged` to avoid loading the entire file.
    /// Filters by height and returns sorted `(height, cmu)` pairs.
    pub fn load_cmus_for_range_paged(
        &self,
        start: u64,
        end: u64,
        page_offset: usize,
        page_limit: usize,
    ) -> Result<Vec<(u32, Vec<u8>)>, StorageError> {
        let outputs = self.load_outputs_paged(page_offset, page_limit)?;
        let mut filtered: Vec<(u32, Vec<u8>)> = outputs
            .into_iter()
            .filter(|o| o.height as u64 >= start && o.height as u64 <= end)
            .map(|o| (o.height, o.cmu))
            .collect();
        filtered.sort_by_key(|(h, _)| *h);
        Ok(filtered)
    }
}

// ---------------------------------------------------------------
// Binary serialization / deserialization
// ---------------------------------------------------------------

/// Detect the record size from the total file size.
/// Returns v2 (684) if divisible, else v1 (652) if divisible, else 0 (corrupt).
fn detect_record_size(file_size: usize) -> usize {
    if file_size == 0 {
        return OUTPUT_RECORD_SIZE; // empty file — use v2 for new writes
    }
    if file_size % OUTPUT_RECORD_SIZE == 0 {
        OUTPUT_RECORD_SIZE // v2 format
    } else if file_size % OUTPUT_RECORD_SIZE_V1 == 0 {
        OUTPUT_RECORD_SIZE_V1 // legacy v1 format
    } else {
        0 // corrupt
    }
}

fn parse_output_record(data: &[u8]) -> DeltaOutput {
    let txid = if data.len() >= OUTPUT_RECORD_SIZE {
        data[652..684].to_vec()
    } else {
        vec![0u8; 32] // v1 records have no txid
    };
    DeltaOutput {
        height: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        index: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
        cmu: data[8..40].to_vec(),
        epk: data[40..72].to_vec(),
        ciphertext: data[72..652].to_vec(),
        txid,
    }
}

fn serialize_output_record(output: &DeltaOutput) -> Vec<u8> {
    let mut record = Vec::with_capacity(OUTPUT_RECORD_SIZE);
    record.extend_from_slice(&output.height.to_le_bytes());
    record.extend_from_slice(&output.index.to_le_bytes());

    // CMU — pad to 32
    let cmu_len = output.cmu.len().min(32);
    record.extend_from_slice(&output.cmu[..cmu_len]);
    if cmu_len < 32 {
        record.extend(std::iter::repeat(0u8).take(32 - cmu_len));
    }

    // EPK — pad to 32
    let epk_len = output.epk.len().min(32);
    record.extend_from_slice(&output.epk[..epk_len]);
    if epk_len < 32 {
        record.extend(std::iter::repeat(0u8).take(32 - epk_len));
    }

    // Ciphertext — pad to 580
    let ct_len = output.ciphertext.len().min(580);
    record.extend_from_slice(&output.ciphertext[..ct_len]);
    if ct_len < 580 {
        record.extend(std::iter::repeat(0u8).take(580 - ct_len));
    }

    // Txid — pad to 32 (v2 field)
    let txid_len = output.txid.len().min(32);
    record.extend_from_slice(&output.txid[..txid_len]);
    if txid_len < 32 {
        record.extend(std::iter::repeat(0u8).take(32 - txid_len));
    }

    debug_assert_eq!(record.len(), OUTPUT_RECORD_SIZE);
    record
}

fn parse_nullifier_record(data: &[u8]) -> DeltaNullifier {
    DeltaNullifier {
        height: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        txid: data[4..36].to_vec(),
        nullifier: data[36..68].to_vec(),
    }
}

fn serialize_nullifier_record(nf: &DeltaNullifier) -> Vec<u8> {
    let mut record = Vec::with_capacity(NULLIFIER_RECORD_SIZE);
    record.extend_from_slice(&nf.height.to_le_bytes());

    let txid_len = nf.txid.len().min(32);
    record.extend_from_slice(&nf.txid[..txid_len]);
    if txid_len < 32 {
        record.extend(std::iter::repeat(0u8).take(32 - txid_len));
    }

    let nf_len = nf.nullifier.len().min(32);
    record.extend_from_slice(&nf.nullifier[..nf_len]);
    if nf_len < 32 {
        record.extend(std::iter::repeat(0u8).take(32 - nf_len));
    }

    debug_assert_eq!(record.len(), NULLIFIER_RECORD_SIZE);
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_store() -> (DeltaCMUStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = DeltaCMUStore::new(dir.path()).unwrap();
        (store, dir)
    }

    fn make_output(height: u32, index: u32) -> DeltaOutput {
        DeltaOutput {
            height,
            index,
            cmu: vec![height as u8; 32],
            epk: vec![index as u8; 32],
            ciphertext: vec![0xCC; 580],
            txid: vec![height as u8; 32],
        }
    }

    #[test]
    fn test_new_store_empty() {
        let (store, _dir) = test_store();
        assert!(!store.has_delta_bundle());
        assert_eq!(store.get_delta_end_height().unwrap(), 0);
        assert_eq!(store.output_count().unwrap(), 0);
    }

    #[test]
    fn test_append_and_load_outputs() {
        let (store, _dir) = test_store();
        let outputs = vec![
            make_output(100, 0),
            make_output(100, 1),
            make_output(200, 0),
        ];

        let appended = store
            .append_outputs(&outputs, 100, 200, Some("aabbcc"))
            .unwrap();
        assert_eq!(appended, 3);
        assert!(store.has_delta_bundle());

        let loaded = store.load_outputs().unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].height, 100);
        assert_eq!(loaded[0].index, 0);
        assert_eq!(loaded[1].height, 100);
        assert_eq!(loaded[1].index, 1);
        assert_eq!(loaded[2].height, 200);
    }

    #[test]
    fn test_append_dedup() {
        let (store, _dir) = test_store();
        let outputs = vec![make_output(100, 0), make_output(100, 1)];
        store.append_outputs(&outputs, 100, 100, None).unwrap();

        // Append overlapping — (100, 0) should be deduped
        let more = vec![make_output(100, 0), make_output(200, 0)];
        let appended = store.append_outputs(&more, 100, 200, None).unwrap();
        assert_eq!(appended, 1); // Only (200, 0) is new
        assert_eq!(store.output_count().unwrap(), 3);
    }

    #[test]
    fn test_load_cmus_sorted() {
        let (store, _dir) = test_store();
        // Insert out of order
        let outputs = vec![
            make_output(300, 0),
            make_output(100, 0),
            make_output(200, 0),
        ];
        store.append_outputs(&outputs, 100, 300, None).unwrap();

        let cmus = store.load_cmus().unwrap();
        assert_eq!(cmus.len(), 3);
        assert_eq!(cmus[0].0, 100); // Sorted by height
        assert_eq!(cmus[1].0, 200);
        assert_eq!(cmus[2].0, 300);
    }

    #[test]
    fn test_load_cmus_for_range() {
        let (store, _dir) = test_store();
        let outputs = vec![
            make_output(100, 0),
            make_output(200, 0),
            make_output(300, 0),
            make_output(400, 0),
        ];
        store.append_outputs(&outputs, 100, 400, None).unwrap();

        let cmus = store.load_cmus_for_range(200, 300).unwrap();
        assert_eq!(cmus.len(), 2);
        assert_eq!(cmus[0].0, 200);
        assert_eq!(cmus[1].0, 300);
    }

    #[test]
    fn test_manifest_roundtrip() {
        let (store, _dir) = test_store();
        let outputs = vec![make_output(100, 0)];
        store
            .append_outputs(&outputs, 100, 100, Some("deadbeef"))
            .unwrap();

        let manifest = store.get_manifest().unwrap().unwrap();
        assert_eq!(manifest.start_height, 100);
        assert_eq!(manifest.end_height, 100);
        assert_eq!(manifest.output_count, 1);
        assert_eq!(manifest.tree_root, Some("deadbeef".to_string()));
    }

    #[test]
    fn test_clear_delta_bundle() {
        let (store, _dir) = test_store();
        let outputs = vec![make_output(100, 0)];
        store.append_outputs(&outputs, 100, 100, None).unwrap();
        assert!(store.has_delta_bundle());

        store.clear_delta_bundle(false, false).unwrap();
        assert!(!store.has_delta_bundle());
        assert_eq!(store.output_count().unwrap(), 0);
    }

    #[test]
    fn test_clear_blocked_when_verified() {
        let (store, _dir) = test_store();
        let outputs = vec![make_output(100, 0)];
        store.append_outputs(&outputs, 100, 100, None).unwrap();

        // Should fail when verified=true, force=false (FIX #1254)
        let result = store.clear_delta_bundle(false, true);
        assert!(result.is_err());
        assert!(store.has_delta_bundle()); // Still exists

        // Should succeed with force=true
        store.clear_delta_bundle(true, true).unwrap();
        assert!(!store.has_delta_bundle());
    }

    #[test]
    fn test_sapling_roots_roundtrip() {
        let (store, _dir) = test_store();
        let entries = vec![(100u64, vec![0xAAu8; 32]), (200u64, vec![0xBBu8; 32])];
        store.append_sapling_roots_batch(&entries).unwrap();

        let loaded = store.load_sapling_roots().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].0, 100);
        assert_eq!(loaded[0].1, vec![0xAAu8; 32]);
        assert_eq!(loaded[1].0, 200);
        assert_eq!(loaded[1].1, vec![0xBBu8; 32]);
    }

    #[test]
    fn test_nullifiers_roundtrip() {
        let (store, _dir) = test_store();
        let nullifiers = vec![
            DeltaNullifier {
                height: 100,
                txid: vec![0x11; 32],
                nullifier: vec![0x22; 32],
            },
            DeltaNullifier {
                height: 200,
                txid: vec![0x33; 32],
                nullifier: vec![0x44; 32],
            },
        ];
        store.append_nullifiers(&nullifiers).unwrap();

        let loaded = store.load_nullifiers().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].height, 100);
        assert_eq!(loaded[0].txid, vec![0x11; 32]);
        assert_eq!(loaded[0].nullifier, vec![0x22; 32]);
        assert_eq!(loaded[1].height, 200);
    }

    #[test]
    fn test_output_record_roundtrip() {
        let output = make_output(12345, 7);
        let serialized = serialize_output_record(&output);
        assert_eq!(serialized.len(), OUTPUT_RECORD_SIZE);

        let parsed = parse_output_record(&serialized);
        assert_eq!(parsed.height, 12345);
        assert_eq!(parsed.index, 7);
        assert_eq!(parsed.cmu, output.cmu);
        assert_eq!(parsed.epk, output.epk);
        assert_eq!(parsed.ciphertext.len(), 580);
        assert_eq!(parsed.txid, output.txid);
    }

    #[test]
    fn test_nullifier_record_roundtrip() {
        let nf = DeltaNullifier {
            height: 99999,
            txid: vec![0xAA; 32],
            nullifier: vec![0xBB; 32],
        };
        let serialized = serialize_nullifier_record(&nf);
        assert_eq!(serialized.len(), NULLIFIER_RECORD_SIZE);

        let parsed = parse_nullifier_record(&serialized);
        assert_eq!(parsed.height, 99999);
        assert_eq!(parsed.txid, nf.txid);
        assert_eq!(parsed.nullifier, nf.nullifier);
    }

    #[test]
    fn test_output_count() {
        let (store, _dir) = test_store();
        assert_eq!(store.output_count().unwrap(), 0);

        let outputs = vec![make_output(100, 0), make_output(200, 0)];
        store.append_outputs(&outputs, 100, 200, None).unwrap();
        assert_eq!(store.output_count().unwrap(), 2);
    }

    #[test]
    fn test_load_outputs_paged() {
        let (store, _dir) = test_store();
        let outputs = vec![
            make_output(100, 0),
            make_output(100, 1),
            make_output(200, 0),
            make_output(300, 0),
            make_output(400, 0),
        ];
        store.append_outputs(&outputs, 100, 400, None).unwrap();

        // Full load
        let all = store.load_outputs_paged(0, 100).unwrap();
        assert_eq!(all.len(), 5);

        // First 2 records
        let page1 = store.load_outputs_paged(0, 2).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].height, 100);
        assert_eq!(page1[0].index, 0);
        assert_eq!(page1[1].height, 100);
        assert_eq!(page1[1].index, 1);

        // Skip first 2, take next 2
        let page2 = store.load_outputs_paged(2, 2).unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].height, 200);
        assert_eq!(page2[1].height, 300);

        // Last record
        let page3 = store.load_outputs_paged(4, 10).unwrap();
        assert_eq!(page3.len(), 1);
        assert_eq!(page3[0].height, 400);

        // Beyond end
        let empty = store.load_outputs_paged(10, 5).unwrap();
        assert!(empty.is_empty());

        // Empty store
        let (store2, _dir2) = test_store();
        let empty2 = store2.load_outputs_paged(0, 10).unwrap();
        assert!(empty2.is_empty());
    }
}
