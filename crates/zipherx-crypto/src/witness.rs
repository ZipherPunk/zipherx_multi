//! Witness operations — merkle path management for spending proofs.
//!
//! A witness is a merkle path from a note's CMU to the tree root.
//! When spending, the anchor (tree root from witness) must exist on the blockchain.
//!
//! CRITICAL INVARIANTS:
//! - Validate EACH witness root, not just tree root
//! - ALWAYS validate witness anchor against HeaderStore after creation
//! - updateAllWitnessesBatch MUST use same size guard as treeAppend

use std::io::Cursor;

use incrementalmerkletree::witness::IncrementalWitness;
use zcash_primitives::merkle_tree::{read_incremental_witness, write_incremental_witness, HashSer};
use zcash_primitives::sapling::Node;

use crate::types::CryptoError;

/// Serialize a witness to bytes.
pub fn serialize_witness(witness: &IncrementalWitness<Node, 32>) -> Result<Vec<u8>, CryptoError> {
    let mut buf = Vec::new();
    write_incremental_witness(witness, &mut buf)
        .map_err(|e| CryptoError::WitnessError(format!("Serialize: {e}")))?;
    Ok(buf)
}

/// Deserialize a witness from bytes.
pub fn deserialize_witness(data: &[u8]) -> Result<IncrementalWitness<Node, 32>, CryptoError> {
    let cursor = Cursor::new(data);
    read_incremental_witness(cursor)
        .map_err(|e| CryptoError::WitnessError(format!("Deserialize: {e}")))
}

/// Get the root hash from a witness (the anchor).
pub fn witness_root(witness_data: &[u8]) -> Result<[u8; 32], CryptoError> {
    let witness = deserialize_witness(witness_data)?;
    let root = witness.root();
    let mut buf = Vec::new();
    root.write(&mut buf)
        .map_err(|e| CryptoError::WitnessError(format!("Root write: {e}")))?;
    let mut result = [0u8; 32];
    result.copy_from_slice(&buf);
    Ok(result)
}

/// Validate that a witness merkle path is structurally valid.
///
/// # Limitations (RCR-13)
///
/// This function only checks that the witness data can be deserialized and that
/// a root can be computed from it. It does NOT verify that the root (anchor)
/// corresponds to any actual blockchain state. Callers must separately validate
/// the witness anchor against the chain's `finalsaplingroot` at the appropriate
/// block height using `verify_anchor()`.
pub fn witness_path_is_valid(witness_data: &[u8]) -> Result<bool, CryptoError> {
    let witness = deserialize_witness(witness_data)?;
    // If we can get the root without error, the path is valid
    let _root = witness.root();
    Ok(true)
}

/// Verify that a witness anchor matches expected root.
pub fn verify_anchor(witness_data: &[u8], expected_root: &[u8; 32]) -> Result<bool, CryptoError> {
    let root = witness_root(witness_data)?;
    Ok(root == *expected_root)
}

/// Update a single witness with a new CMU.
///
/// Returns the updated witness bytes.
pub fn update_witness(witness_data: &[u8], cmu: &[u8; 32]) -> Result<Vec<u8>, CryptoError> {
    let mut witness = deserialize_witness(witness_data)?;

    let node = Node::read(&cmu[..]).map_err(|_| CryptoError::InvalidCommitment)?;

    witness
        .append(node)
        .map_err(|e| CryptoError::WitnessError(format!("Witness append failed: {:?}", e)))?;

    serialize_witness(&witness)
}

/// Update a witness with a batch of CMUs.
///
/// Returns the updated witness bytes.
pub fn update_witness_batch(witness_data: &[u8], cmus: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if cmus.len() % 32 != 0 {
        return Err(CryptoError::InvalidData(format!(
            "CMU data length {} not a multiple of 32",
            cmus.len()
        )));
    }

    let mut witness = deserialize_witness(witness_data)?;
    let count = cmus.len() / 32;

    for i in 0..count {
        let cmu_slice = &cmus[i * 32..(i + 1) * 32];
        let node = Node::read(cmu_slice).map_err(|_| CryptoError::InvalidCommitment)?;
        witness.append(node).map_err(|e| {
            CryptoError::WitnessError(format!("Witness append failed in batch: {:?}", e))
        })?;
    }

    serialize_witness(&witness)
}
