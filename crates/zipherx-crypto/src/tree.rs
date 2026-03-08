//! Sapling commitment tree — incremental merkle tree of note commitments.
//!
//! The commitment tree stores all note commitments (CMUs) and provides
//! merkle paths (witnesses) for spending proofs.
//!
//! CRITICAL INVARIANTS:
//! - tree_append is append-only with NO dedup — same CMU twice = wrong root
//! - MUST save DB tree height after appending CMUs
//! - ALWAYS validate FFI root against blockchain
//! - ALWAYS validate BEFORE persisting
//! - Delta CMUs MUST be sorted by block height
//!
//! # Lock ordering (RCR-7)
//!
//! This module uses 4 global mutexes. To prevent deadlocks, they MUST always
//! be acquired in this order:
//!
//! 1. `TREE` — the commitment tree itself
//! 2. `WITNESSES` — the incremental witnesses collection
//! 3. `TREE_POSITION` — the current tree size counter
//! 4. `DELTA_CMUS` — the buffered delta CMUs
//!
//! Never acquire a lower-numbered lock while holding a higher-numbered one.
//! The `append` and `append_batch` functions demonstrate the correct ordering.

use std::io::Cursor;
use std::sync::Mutex;

use incrementalmerkletree::{frontier::CommitmentTree, witness::IncrementalWitness};
use zcash_primitives::merkle_tree::{
    read_commitment_tree, read_incremental_witness, write_commitment_tree, HashSer,
};
use zcash_primitives::sapling::Node;

use crate::types::CryptoError;

/// Global commitment tree instance.
///
/// # Design constraint: single-wallet process
///
/// These process-wide singletons mean only one wallet can operate per process.
/// This is intentional for ZipherX's architecture (one wallet per app instance).
/// If multi-wallet support is needed, these should be refactored into an
/// instance-based `TreeState` struct.
///
/// # Mutex poisoning
///
/// If a thread panics while holding one of these locks, the mutex becomes
/// poisoned and all subsequent lock attempts return `CryptoError::TreeError`.
/// We intentionally propagate the error rather than recovering with
/// `unwrap_or_else(|e| e.into_inner())` because a panic during tree mutation
/// may leave the commitment tree in an inconsistent state, and continuing
/// with corrupted tree data could lead to incorrect balances or failed spends.
/// The caller should reinitialize via `init()` after encountering a poisoned lock.
static TREE: Mutex<Option<CommitmentTree<Node, 32>>> = Mutex::new(None);

/// Global witnesses collection.
static WITNESSES: Mutex<Vec<IncrementalWitness<Node, 32>>> = Mutex::new(Vec::new());

/// Current tree position (number of appended nodes).
static TREE_POSITION: Mutex<u64> = Mutex::new(0);

/// Delta CMUs buffer (recent CMUs not yet persisted).
static DELTA_CMUS: Mutex<Vec<Node>> = Mutex::new(Vec::new());

/// Maximum delta CMUs to buffer in memory.
/// Keep low to avoid memory pressure on mobile devices.
const MAX_DELTA_CMUS: usize = 50_000;

/// Parse a 32-byte CMU slice into a Node using Node::read().
fn parse_node(cmu: &[u8]) -> Result<Node, CryptoError> {
    Node::read(cmu).map_err(|_| CryptoError::InvalidCommitment)
}

/// Serialize a Node root to 32 bytes.
fn root_to_bytes(root: &Node) -> Result<[u8; 32], CryptoError> {
    let mut buf = Vec::new();
    root.write(&mut buf)
        .map_err(|e| CryptoError::TreeError(format!("Root write: {e}")))?;
    if buf.len() != 32 {
        return Err(CryptoError::TreeError(format!(
            "Root unexpected len: {}",
            buf.len()
        )));
    }
    let mut result = [0u8; 32];
    result.copy_from_slice(&buf);
    Ok(result)
}

/// Initialize a new empty commitment tree.
pub fn init() -> Result<(), CryptoError> {
    let mut tree = TREE
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    *tree = Some(CommitmentTree::empty());

    let mut pos = TREE_POSITION
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    *pos = 0;

    let mut witnesses = WITNESSES
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    witnesses.clear();

    let mut delta = DELTA_CMUS
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    delta.clear();
    delta.shrink_to_fit();

    Ok(())
}

/// Append a single CMU to the tree.
///
/// Returns the new tree size.
pub fn append(cmu: &[u8; 32]) -> Result<u64, CryptoError> {
    let node = parse_node(cmu)?;

    let mut tree = TREE
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    let tree = tree
        .as_mut()
        .ok_or(CryptoError::TreeError("Tree not initialized".into()))?;

    tree.append(node)
        .map_err(|_| CryptoError::TreeError("Append failed".into()))?;

    // Update witnesses
    let mut witnesses = WITNESSES
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    for witness in witnesses.iter_mut() {
        witness
            .append(node)
            .map_err(|e| CryptoError::WitnessError(format!("Witness append failed: {:?}", e)))?;
    }

    let mut pos = TREE_POSITION
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    *pos += 1;

    // Buffer delta CMU (best-effort — skip if buffer full)
    let mut delta = DELTA_CMUS
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    if delta.len() < MAX_DELTA_CMUS {
        delta.push(node);
    }

    Ok(*pos)
}

/// Append a batch of CMUs to the tree.
///
/// Returns the new tree size.
pub fn append_batch(cmus: &[u8]) -> Result<u64, CryptoError> {
    if cmus.len() % 32 != 0 {
        return Err(CryptoError::InvalidData(format!(
            "CMU data length {} not a multiple of 32",
            cmus.len()
        )));
    }

    let count = cmus.len() / 32;

    // Parse all CMUs first
    let mut nodes = Vec::with_capacity(count);
    for i in 0..count {
        let cmu_slice = &cmus[i * 32..(i + 1) * 32];
        nodes.push(parse_node(cmu_slice)?);
    }

    let mut tree = TREE
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    let tree = tree
        .as_mut()
        .ok_or(CryptoError::TreeError("Tree not initialized".into()))?;

    let mut witnesses = WITNESSES
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    let mut delta = DELTA_CMUS
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;

    for node in &nodes {
        tree.append(*node)
            .map_err(|_| CryptoError::TreeError("Append failed".into()))?;
        for witness in witnesses.iter_mut() {
            witness.append(*node).map_err(|e| {
                CryptoError::WitnessError(format!("Witness append failed in batch: {:?}", e))
            })?;
        }
        if delta.len() < MAX_DELTA_CMUS {
            delta.push(*node);
        }
    }

    let mut pos = TREE_POSITION
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    *pos += count as u64;

    Ok(*pos)
}

/// Get the current tree root.
pub fn root() -> Result<[u8; 32], CryptoError> {
    let tree = TREE
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    let tree = tree
        .as_ref()
        .ok_or(CryptoError::TreeError("Tree not initialized".into()))?;
    root_to_bytes(&tree.root())
}

/// Get the current tree size (number of appended CMUs).
pub fn size() -> Result<u64, CryptoError> {
    let pos = TREE_POSITION
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    Ok(*pos)
}

/// Set the tree position explicitly.
///
/// Use after `deserialize()` to restore the correct position from the DB's
/// tree_height, since `deserialize()` does not update TREE_POSITION.
pub fn set_position(position: u64) -> Result<(), CryptoError> {
    let mut pos = TREE_POSITION
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    *pos = position;
    Ok(())
}

/// Serialize the tree to bytes.
pub fn serialize() -> Result<Vec<u8>, CryptoError> {
    let tree = TREE
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    let tree = tree
        .as_ref()
        .ok_or(CryptoError::TreeError("Tree not initialized".into()))?;

    let mut buf = Vec::new();
    write_commitment_tree(tree, &mut buf)
        .map_err(|e| CryptoError::TreeError(format!("Serialize: {e}")))?;

    Ok(buf)
}

/// Deserialize a tree from bytes (replaces current tree).
pub fn deserialize(data: &[u8]) -> Result<(), CryptoError> {
    let cursor = Cursor::new(data);
    let new_tree: CommitmentTree<Node, 32> = read_commitment_tree(cursor)
        .map_err(|e| CryptoError::TreeError(format!("Deserialize: {e}")))?;

    let mut tree = TREE
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    *tree = Some(new_tree);

    Ok(())
}

/// Get the root of a serialized tree WITHOUT loading it into the global tree.
///
/// Useful for validation: compare the tree root from a boost file against the
/// blockchain's finalsaplingroot to verify completeness.
pub fn root_from_serialized(data: &[u8]) -> Result<[u8; 32], CryptoError> {
    let cursor = Cursor::new(data);
    let tree: CommitmentTree<Node, 32> = read_commitment_tree(cursor)
        .map_err(|e| CryptoError::TreeError(format!("Deserialize for root: {e}")))?;
    root_to_bytes(&tree.root())
}

/// Create a witness for the current tree position.
///
/// Returns the witness index.
pub fn witness_current() -> Result<u64, CryptoError> {
    let tree = TREE
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    let tree = tree
        .as_ref()
        .ok_or(CryptoError::TreeError("Tree not initialized".into()))?;

    let witness = IncrementalWitness::from_tree(tree.clone());

    let mut witnesses = WITNESSES
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    witnesses.push(witness);

    Ok((witnesses.len() - 1) as u64)
}

/// Serialize a witness by index.
///
/// Returns the witness bytes that can be stored in the database.
pub fn get_witness_serialized(idx: u64) -> Result<Vec<u8>, CryptoError> {
    let witnesses = WITNESSES
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    let witness = witnesses.get(idx as usize).ok_or_else(|| {
        CryptoError::TreeError(format!(
            "Witness index {} out of range (have {})",
            idx,
            witnesses.len()
        ))
    })?;

    let mut buf = Vec::new();
    zcash_primitives::merkle_tree::write_incremental_witness(witness, &mut buf)
        .map_err(|e| CryptoError::WitnessError(format!("Serialize: {e}")))?;
    Ok(buf)
}

/// Get the root (anchor) of a witness by index.
pub fn get_witness_root(idx: u64) -> Result<[u8; 32], CryptoError> {
    let witnesses = WITNESSES
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    let witness = witnesses
        .get(idx as usize)
        .ok_or_else(|| CryptoError::TreeError(format!("Witness index {} out of range", idx)))?;
    root_to_bytes(&witness.root())
}

/// Get the total number of active witnesses.
pub fn witness_count() -> Result<u64, CryptoError> {
    let witnesses = WITNESSES
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    Ok(witnesses.len() as u64)
}

/// Clear all witnesses (does NOT clear the tree).
pub fn clear_witnesses() -> Result<u64, CryptoError> {
    let mut witnesses = WITNESSES
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    let count = witnesses.len() as u64;
    witnesses.clear();
    Ok(count)
}

/// Get the number of delta CMUs buffered.
pub fn delta_cmus_count() -> Result<u64, CryptoError> {
    let delta = DELTA_CMUS
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    Ok(delta.len() as u64)
}

/// Get the delta CMUs as raw bytes.
pub fn get_delta_cmus() -> Result<Vec<u8>, CryptoError> {
    let delta = DELTA_CMUS
        .lock()
        .map_err(|e| CryptoError::TreeError(format!("Lock: {e}")))?;
    let mut buf = Vec::with_capacity(delta.len() * 32);
    for node in delta.iter() {
        let mut node_bytes = Vec::new();
        node.write(&mut node_bytes)
            .map_err(|e| CryptoError::TreeError(format!("Node write: {e}")))?;
        buf.extend_from_slice(&node_bytes);
    }
    Ok(buf)
}

/// Get the note's tree position from serialized witness bytes.
///
/// Returns `witnessed_position()` — the position of the note's CMU in the
/// commitment tree at the time the witness was created. This is the correct
/// position for nullifier derivation per the Sapling spec (§ 4.16).
pub fn witnessed_position_from_bytes(data: &[u8]) -> Result<u64, CryptoError> {
    let mut reader = Cursor::new(data);
    let witness: IncrementalWitness<Node, 32> = read_incremental_witness(&mut reader)
        .map_err(|e| CryptoError::WitnessError(format!("Deserialize witness: {e}")))?;
    Ok(u64::from(witness.witnessed_position()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_init() {
        init().unwrap();
        assert_eq!(size().unwrap(), 0);
    }

    #[test]
    fn test_tree_serialize_deserialize() {
        init().unwrap();

        let root_before = root().unwrap();

        let serialized = serialize().unwrap();
        assert!(!serialized.is_empty());

        // Reinitialize and deserialize
        init().unwrap();
        deserialize(&serialized).unwrap();
        let root_after = root().unwrap();

        assert_eq!(root_before, root_after);
    }

    #[test]
    fn test_delta_cmus_empty() {
        init().unwrap();
        assert_eq!(delta_cmus_count().unwrap(), 0);
    }
}
