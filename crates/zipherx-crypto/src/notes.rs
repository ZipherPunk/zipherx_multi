//! Sapling note operations — trial decryption, nullifier, and CMU computation.
//!
//! Notes are the fundamental unit of value in Sapling:
//! - A note = (d, pk_d, v, rcm, memo)
//! - CMU = NoteCommit_rcm(g_d, pk_d, v) — the note commitment
//! - Nullifier = PRF_nk(ρ) — reveals spending without revealing the note
//!
//! Two decryption strategies:
//! - IVK-based: Manual ChaCha20Poly1305 (fast, used for block scanning)
//! - SK-based: Via zcash_primitives `try_sapling_note_decryption` (authoritative)

use zcash_primitives::{
    consensus::BlockHeight,
    sapling::{
        keys::{FullViewingKey, OutgoingViewingKey, SaplingIvk},
        note_encryption::{
            try_sapling_note_decryption, PreparedIncomingViewingKey, SaplingDomain,
        },
        value::NoteValue,
        Diversifier, Rseed,
    },
    zip32::sapling::ExtendedSpendingKey,
};
use zcash_note_encryption::{EphemeralKeyBytes, ShieldedOutput, ENC_CIPHERTEXT_SIZE};

use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce, aead::Aead, KeyInit};
use ff::PrimeField;
use group::{cofactor::CofactorGroup, Curve, GroupEncoding};
use rayon::prelude::*;

use crate::types::{
    ZclassicNetwork, CryptoError, SPENDING_KEY_LENGTH, ENC_CIPHERTEXT_LEN,
};

// ============================================================================
// Types
// ============================================================================

/// A successfully decrypted Sapling note.
#[derive(Debug, Clone)]
pub struct DecryptedNote {
    /// Diversifier (11 bytes).
    pub diversifier: [u8; 11],
    /// Note value in zatoshis.
    pub value: u64,
    /// Randomness commitment (32 bytes) — raw rcm for BeforeZip212, raw rseed for AfterZip212.
    pub rcm: [u8; 32],
    /// Whether this note uses ZIP-212 (AfterZip212) rseed format.
    /// If true, `rcm` contains the rseed and actual rcm is PRF-derived.
    pub is_zip212: bool,
    /// Memo field (512 bytes).
    pub memo: Vec<u8>,
}

/// Internal struct implementing ShieldedOutput for zcash_primitives decryption.
struct RawShieldedOutput {
    epk: [u8; 32],
    cmu: [u8; 32],
    enc_ciphertext: [u8; 580],
}

impl<P: zcash_primitives::consensus::Parameters> ShieldedOutput<SaplingDomain<P>, ENC_CIPHERTEXT_SIZE>
    for RawShieldedOutput
{
    fn ephemeral_key(&self) -> EphemeralKeyBytes {
        EphemeralKeyBytes(self.epk)
    }

    fn cmstar_bytes(&self) -> [u8; 32] {
        self.cmu
    }

    fn enc_ciphertext(&self) -> &[u8; ENC_CIPHERTEXT_SIZE] {
        &self.enc_ciphertext
    }
}

// ============================================================================
// Trial Decryption (IVK-based, manual ChaCha20Poly1305)
// ============================================================================

/// Try to decrypt a Sapling note using the incoming viewing key.
///
/// This is the fast path used during block scanning. Performs:
/// 1. Ka = [8 * ivk] * epk  (cofactor clearing)
/// 2. KDF via BLAKE2b("Zcash_SaplingKDF", Ka || epk)
/// 3. ChaCha20Poly1305 decrypt
///
/// Returns `None` if decryption fails (note not addressed to this IVK).
pub fn try_decrypt_note(
    ivk: &[u8; 32],
    epk: &[u8; 32],
    cmu: &[u8; 32],
    ciphertext: &[u8],
) -> Option<DecryptedNote> {
    if ciphertext.len() < ENC_CIPHERTEXT_LEN {
        return None;
    }

    // Parse IVK as jubjub scalar
    let ivk_scalar: jubjub::Fr = Option::from(jubjub::Fr::from_repr(*ivk))?;

    // Parse EPK as curve point
    let epk_point: jubjub::ExtendedPoint =
        Option::from(jubjub::ExtendedPoint::from_bytes(epk))?;

    // Key agreement: Ka = [8 * ivk] * epk (cofactor clearing)
    let ka: jubjub::ExtendedPoint = epk_point * ivk_scalar;
    let ka_cleared = ka.clear_cofactor();
    let ka_extended: jubjub::ExtendedPoint = ka_cleared.into();
    let ka_bytes = ka_extended.to_affine().to_bytes();

    // KDF: BLAKE2b-256("Zcash_SaplingKDF", Ka || epk)
    let symmetric_key = blake2b_simd::Params::new()
        .hash_length(32)
        .personal(b"Zcash_SaplingKDF")
        .to_state()
        .update(&ka_bytes)
        .update(epk)
        .finalize();

    // ChaCha20Poly1305 decrypt (580 = 564 plaintext + 16 tag)
    let key = Key::from_slice(symmetric_key.as_bytes());
    let cipher = ChaCha20Poly1305::new(key);
    let nonce = Nonce::from_slice(&[0u8; 12]);

    let decrypted = cipher.decrypt(nonce, &ciphertext[..ENC_CIPHERTEXT_LEN]).ok()?;

    if decrypted.len() < 51 {
        return None;
    }

    // Parse: diversifier(11) + value(8) + rcm(32) + memo(up to 512)
    let mut diversifier = [0u8; 11];
    diversifier.copy_from_slice(&decrypted[0..11]);

    let value = u64::from_le_bytes(
        decrypted[11..19].try_into().ok()?,
    );

    let mut rcm = [0u8; 32];
    rcm.copy_from_slice(&decrypted[19..51]);

    let memo = if decrypted.len() > 51 {
        decrypted[51..].to_vec()
    } else {
        vec![0u8; 512]
    };

    // RCR-2: Verify the note commitment (CMU) matches the provided cmu.
    // Reconstruct the note from decrypted components and verify its CMU
    // to prevent ciphertext substitution attacks.
    {
        let sapling_ivk = SaplingIvk(ivk_scalar);
        let div = Diversifier(diversifier);
        let payment_address = sapling_ivk.to_payment_address(div)?;

        let rcm_scalar: jubjub::Fr = Option::from(jubjub::Fr::from_repr(rcm))?;
        let note = zcash_primitives::sapling::Note::from_parts(
            payment_address,
            NoteValue::from_raw(value),
            Rseed::BeforeZip212(rcm_scalar),
        );
        let note_cmu = note.cmu();
        if note_cmu.to_bytes() != *cmu {
            return None; // CMU mismatch — decryption may be spurious
        }
    }

    Some(DecryptedNote { diversifier, value, rcm, is_zip212: false, memo })
}

// ============================================================================
// Trial Decryption (SK-based, via zcash_primitives)
// ============================================================================

/// Try to decrypt a Sapling note using the spending key via zcash_primitives.
///
/// This is the authoritative decryption path that uses the full
/// `try_sapling_note_decryption` from zcash_primitives. Slower but
/// validates the note against the Sapling protocol spec.
///
/// Returns `None` if decryption fails.
///
/// # Security (RCR-6)
///
/// This function deserializes the spending key from `sk_bytes`. The `extsk`
/// is dropped at end of scope. Callers SHOULD wrap `sk_bytes` in
/// `zeroize::Zeroizing<Vec<u8>>` to ensure key material is securely zeroed.
pub fn try_decrypt_note_with_sk(
    sk_bytes: &[u8],
    epk: &[u8; 32],
    cmu: &[u8; 32],
    ciphertext: &[u8],
    height: u64,
) -> Option<DecryptedNote> {
    if sk_bytes.len() != SPENDING_KEY_LENGTH || ciphertext.len() < ENC_CIPHERTEXT_LEN {
        return None;
    }

    let extsk = ExtendedSpendingKey::read(&mut &sk_bytes[..]).ok()?;
    let fvk = FullViewingKey::from_expanded_spending_key(&extsk.expsk);
    let ivk = fvk.vk.ivk();
    let prepared_ivk = PreparedIncomingViewingKey::new(&ivk);

    let mut enc = [0u8; ENC_CIPHERTEXT_LEN];
    enc.copy_from_slice(&ciphertext[..ENC_CIPHERTEXT_LEN]);

    let output = RawShieldedOutput {
        epk: *epk,
        cmu: *cmu,
        enc_ciphertext: enc,
    };

    if height > u32::MAX as u64 {
        return None; // Height exceeds u32 range
    }
    let block_height = BlockHeight::from_u32(height as u32);

    let (note, address, memo) =
        try_sapling_note_decryption(&ZclassicNetwork, block_height, &prepared_ivk, &output)?;

    let diversifier = address.diversifier().0;
    let value: u64 = note.value().inner();
    let is_zip212 = matches!(note.rseed(), Rseed::AfterZip212(_));
    let rcm = match note.rseed() {
        Rseed::BeforeZip212(rcm) => rcm.to_repr(),
        Rseed::AfterZip212(rseed) => *rseed,
    };

    Some(DecryptedNote {
        diversifier,
        value,
        rcm,
        is_zip212,
        memo: memo.as_array().to_vec(),
    })
}

// ============================================================================
// Parallel Decryption (Rayon batch)
// ============================================================================

/// Batch decrypt multiple shielded outputs in parallel using Rayon.
///
/// Input format per output: epk(32) + cmu(32) + ciphertext(580) = 644 bytes.
///
/// Returns `(index, DecryptedNote)` for each successfully decrypted output.
pub fn try_decrypt_notes_parallel(
    sk_bytes: &[u8],
    outputs_data: &[u8],
    height: u64,
) -> Vec<(usize, DecryptedNote)> {
    const RECORD_SIZE: usize = 644; // epk(32) + cmu(32) + ciphertext(580)

    if sk_bytes.len() != SPENDING_KEY_LENGTH {
        return Vec::new();
    }

    let output_count = outputs_data.len() / RECORD_SIZE;
    if output_count == 0 {
        return Vec::new();
    }

    let extsk = match ExtendedSpendingKey::read(&mut &sk_bytes[..]) {
        Ok(k) => k,
        Err(_) => return Vec::new(),
    };

    let fvk = FullViewingKey::from_expanded_spending_key(&extsk.expsk);
    let ivk = fvk.vk.ivk();
    let prepared_ivk = PreparedIncomingViewingKey::new(&ivk);
    if height > u32::MAX as u64 {
        return Vec::new(); // Height exceeds u32 range
    }
    let block_height = BlockHeight::from_u32(height as u32);

    (0..output_count)
        .into_par_iter()
        .filter_map(|i| {
            let offset = i * RECORD_SIZE;
            let mut epk = [0u8; 32];
            let mut cmu = [0u8; 32];
            let mut enc = [0u8; ENC_CIPHERTEXT_LEN];
            epk.copy_from_slice(&outputs_data[offset..offset + 32]);
            cmu.copy_from_slice(&outputs_data[offset + 32..offset + 64]);
            enc.copy_from_slice(&outputs_data[offset + 64..offset + 64 + ENC_CIPHERTEXT_LEN]);

            let output = RawShieldedOutput {
                epk,
                cmu,
                enc_ciphertext: enc,
            };

            let (note, address, memo) = try_sapling_note_decryption(
                &ZclassicNetwork,
                block_height,
                &prepared_ivk,
                &output,
            )?;

            let diversifier = address.diversifier().0;
            let value: u64 = note.value().inner();
            let is_zip212 = matches!(note.rseed(), Rseed::AfterZip212(_));
            let rcm = match note.rseed() {
                Rseed::BeforeZip212(rcm) => rcm.to_repr(),
                Rseed::AfterZip212(rseed) => *rseed,
            };

            Some((i, DecryptedNote {
                diversifier,
                value,
                rcm,
                is_zip212,
                memo: memo.as_array().to_vec(),
            }))
        })
        .collect()
}

// ============================================================================
// Nullifier Computation
// ============================================================================

/// Compute the nullifier for a note.
///
/// nf = PRF_nk(ρ) where ρ depends on the note position in the tree.
///
/// # Arguments
/// * `sk_bytes` - ExtendedSpendingKey (169 bytes)
/// * `diversifier` - Note diversifier (11 bytes)
/// * `value` - Note value in zatoshis
/// * `rcm` - Note randomness commitment (32 bytes — raw rcm for BeforeZip212, rseed for AfterZip212)
/// * `position` - Note position in the commitment tree
/// * `is_zip212` - Whether the note uses ZIP-212 (AfterZip212) rseed format
pub fn compute_nullifier(
    sk_bytes: &[u8],
    diversifier: &[u8; 11],
    value: u64,
    rcm: &[u8; 32],
    position: u64,
    is_zip212: bool,
) -> Result<[u8; 32], CryptoError> {
    if sk_bytes.len() != SPENDING_KEY_LENGTH {
        return Err(CryptoError::InvalidSpendingKey);
    }

    let extsk = ExtendedSpendingKey::read(&mut &sk_bytes[..])
        .map_err(|_| CryptoError::InvalidSpendingKey)?;

    let dfvk = extsk.to_diversifiable_full_viewing_key();
    let nk = dfvk.fvk().vk.nk;

    let div = Diversifier(*diversifier);
    let payment_address = dfvk
        .fvk()
        .vk
        .to_payment_address(div)
        .ok_or(CryptoError::InvalidDiversifier(0))?;

    let note = if is_zip212 {
        payment_address.create_note(value, Rseed::AfterZip212(*rcm))
    } else {
        let rcm_scalar = jubjub::Fr::from_repr(*rcm)
            .into_option()
            .ok_or(CryptoError::InvalidData("Invalid rcm scalar".into()))?;
        payment_address.create_note(value, Rseed::BeforeZip212(rcm_scalar))
    };
    let nullifier = note.nf(&nk, position);

    Ok(nullifier.0)
}

// ============================================================================
// CMU Computation
// ============================================================================

/// Compute the note commitment (CMU) from note components.
///
/// CMU = NoteCommit_rcm(g_d, pk_d, v) on the jubjub curve.
///
/// Requires the spending key to derive the payment address from the diversifier.
pub fn compute_cmu(
    sk_bytes: &[u8],
    diversifier: &[u8; 11],
    value: u64,
    rcm: &[u8; 32],
    is_zip212: bool,
) -> Result<[u8; 32], CryptoError> {
    if sk_bytes.len() != SPENDING_KEY_LENGTH {
        return Err(CryptoError::InvalidSpendingKey);
    }

    let extsk = ExtendedSpendingKey::read(&mut &sk_bytes[..])
        .map_err(|_| CryptoError::InvalidSpendingKey)?;

    let fvk = extsk.to_diversifiable_full_viewing_key();
    let div = Diversifier(*diversifier);
    let note_addr = fvk
        .fvk()
        .vk
        .to_payment_address(div)
        .ok_or(CryptoError::InvalidDiversifier(0))?;

    let note = if is_zip212 {
        zcash_primitives::sapling::Note::from_parts(
            note_addr,
            NoteValue::from_raw(value),
            Rseed::AfterZip212(*rcm),
        )
    } else {
        let rcm_scalar = jubjub::Fr::from_repr(*rcm)
            .into_option()
            .ok_or(CryptoError::InvalidData("Invalid rcm scalar".into()))?;
        zcash_primitives::sapling::Note::from_parts(
            note_addr,
            NoteValue::from_raw(value),
            Rseed::BeforeZip212(rcm_scalar),
        )
    };

    let cmu = note.cmu();
    Ok(cmu.to_bytes())
}

/// Verify that a stored CMU matches the computed CMU from note components.
///
/// Returns `true` if the stored CMU matches the canonical computed CMU.
///
/// ## RCR-11: Canonical CMU only
///
/// Only the canonical byte order is accepted. The previous reversed byte order
/// fallback has been removed to prevent accepting notes with incorrect CMU
/// encoding, which could mask data corruption.
///
/// ## RCR-NEW-6: Timing side-channel note
///
/// This comparison uses standard `==` (variable-time). For CMU/nullifier matching
/// this is acceptable because: (1) CMU values are public on-chain, and (2) the
/// comparison is between locally-computed values, not attacker-controlled secrets.
/// A future improvement could use `subtle::ConstantTimeEq` if the `subtle` crate
/// is re-added to this crate's dependencies.
pub fn verify_note_cmu(
    sk_bytes: &[u8],
    diversifier: &[u8; 11],
    value: u64,
    rcm: &[u8; 32],
    expected_cmu: &[u8; 32],
    is_zip212: bool,
) -> Result<bool, CryptoError> {
    let computed = compute_cmu(sk_bytes, diversifier, value, rcm, is_zip212)?;
    Ok(computed == *expected_cmu)
}

// ============================================================================
// OVK Recovery (for sent transaction outputs)
// ============================================================================

/// Try to recover a sent note using the outgoing viewing key.
///
/// Used to detect change outputs and sent amounts in our own transactions.
/// Requires the OVK, value commitment (cv), and both enc/out ciphertexts.
pub fn try_recover_output_with_ovk(
    ovk: &[u8; 32],
    cv: &[u8; 32],
    cmu: &[u8; 32],
    epk: &[u8; 32],
    enc_ciphertext: &[u8; 580],
    out_ciphertext: &[u8; 80],
    height: u64,
) -> Option<DecryptedNote> {
    use zcash_primitives::sapling::value::ValueCommitment;

    let ovk_obj = OutgoingViewingKey(*ovk);
    let cv_obj = ValueCommitment::from_bytes_not_small_order(cv).into_option()?;
    let cmu_obj =
        zcash_primitives::sapling::note::ExtractedNoteCommitment::from_bytes(cmu).into_option()?;
    let epk_obj = EphemeralKeyBytes(*epk);

    // Build recovery output struct
    struct RecoveryOutput {
        cv: zcash_primitives::sapling::value::ValueCommitment,
        cmu: zcash_primitives::sapling::note::ExtractedNoteCommitment,
        epk: EphemeralKeyBytes,
        enc: [u8; 580],
        out: [u8; 80],
    }

    impl ShieldedOutput<SaplingDomain<ZclassicNetwork>, 580> for RecoveryOutput {
        fn ephemeral_key(&self) -> EphemeralKeyBytes {
            self.epk.clone()
        }
        fn cmstar_bytes(&self) -> [u8; 32] {
            self.cmu.to_bytes()
        }
        fn enc_ciphertext(&self) -> &[u8; 580] {
            &self.enc
        }
    }

    let recovery_output = RecoveryOutput {
        cv: cv_obj,
        cmu: cmu_obj,
        epk: epk_obj,
        enc: *enc_ciphertext,
        out: *out_ciphertext,
    };

    if height > u32::MAX as u64 {
        return None; // Height exceeds u32 range
    }
    let block_height = BlockHeight::from_u32(height as u32);
    let domain = SaplingDomain::for_height(ZclassicNetwork, block_height);

    let (note, address, memo) = zcash_note_encryption::try_output_recovery_with_ovk(
        &domain,
        &ovk_obj,
        &recovery_output,
        &recovery_output.cv,
        &recovery_output.out,
    )?;

    let diversifier = address.diversifier().0;
    let value: u64 = note.value().inner();
    let is_zip212 = matches!(note.rseed(), Rseed::AfterZip212(_));
    let rcm = match note.rseed() {
        Rseed::BeforeZip212(rcm) => rcm.to_repr(),
        Rseed::AfterZip212(rseed) => *rseed,
    };

    Some(DecryptedNote {
        diversifier,
        value,
        rcm,
        is_zip212,
        memo: memo.as_array().to_vec(),
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{keys, mnemonic};

    fn test_sk() -> Vec<u8> {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        let seed = mnemonic::to_seed(phrase).unwrap();
        keys::derive_spending_key(&seed, 0).unwrap().to_vec()
    }

    #[test]
    fn test_compute_nullifier_basic() {
        let sk = test_sk();
        let (addr, _) = keys::derive_address(&sk, 0).unwrap();

        // Extract diversifier from address bytes
        let mut diversifier = [0u8; 11];
        diversifier.copy_from_slice(&addr[0..11]);

        // Use a test rcm (valid scalar — all zeros is the identity)
        let rcm = [0u8; 32];

        // compute_nullifier requires a valid rcm scalar.
        // All-zeros is the zero element of Fr, which is valid.
        let result = compute_nullifier(&sk, &diversifier, 100_000, &rcm, 0, false);
        assert!(result.is_ok());
        let nf = result.unwrap();
        assert_ne!(nf, [0u8; 32]); // Nullifier should not be all zeros
    }

    #[test]
    fn test_compute_nullifier_deterministic() {
        let sk = test_sk();
        let (addr, _) = keys::derive_address(&sk, 0).unwrap();
        let mut diversifier = [0u8; 11];
        diversifier.copy_from_slice(&addr[0..11]);
        let rcm = [0u8; 32];

        let nf1 = compute_nullifier(&sk, &diversifier, 100_000, &rcm, 42, false).unwrap();
        let nf2 = compute_nullifier(&sk, &diversifier, 100_000, &rcm, 42, false).unwrap();
        assert_eq!(nf1, nf2);
    }

    #[test]
    fn test_compute_nullifier_different_positions() {
        let sk = test_sk();
        let (addr, _) = keys::derive_address(&sk, 0).unwrap();
        let mut diversifier = [0u8; 11];
        diversifier.copy_from_slice(&addr[0..11]);
        let rcm = [0u8; 32];

        let nf1 = compute_nullifier(&sk, &diversifier, 100_000, &rcm, 0, false).unwrap();
        let nf2 = compute_nullifier(&sk, &diversifier, 100_000, &rcm, 1, false).unwrap();
        assert_ne!(nf1, nf2);
    }

    #[test]
    fn test_compute_cmu_basic() {
        let sk = test_sk();
        let (addr, _) = keys::derive_address(&sk, 0).unwrap();
        let mut diversifier = [0u8; 11];
        diversifier.copy_from_slice(&addr[0..11]);
        let rcm = [0u8; 32];

        let cmu = compute_cmu(&sk, &diversifier, 100_000, &rcm, false).unwrap();
        assert_ne!(cmu, [0u8; 32]);
    }

    #[test]
    fn test_verify_note_cmu_matches() {
        let sk = test_sk();
        let (addr, _) = keys::derive_address(&sk, 0).unwrap();
        let mut diversifier = [0u8; 11];
        diversifier.copy_from_slice(&addr[0..11]);
        let rcm = [0u8; 32];

        let cmu = compute_cmu(&sk, &diversifier, 100_000, &rcm, false).unwrap();
        let verified = verify_note_cmu(&sk, &diversifier, 100_000, &rcm, &cmu, false).unwrap();
        assert!(verified);
    }

    #[test]
    fn test_verify_note_cmu_reversed_no_match() {
        // RCR-11: Reversed byte order is no longer accepted (canonical only).
        let sk = test_sk();
        let (addr, _) = keys::derive_address(&sk, 0).unwrap();
        let mut diversifier = [0u8; 11];
        diversifier.copy_from_slice(&addr[0..11]);
        let rcm = [0u8; 32];

        let cmu = compute_cmu(&sk, &diversifier, 100_000, &rcm, false).unwrap();

        // Reverse the CMU bytes
        let mut reversed = [0u8; 32];
        for i in 0..32 {
            reversed[i] = cmu[31 - i];
        }

        let verified = verify_note_cmu(&sk, &diversifier, 100_000, &rcm, &reversed, false).unwrap();
        assert!(!verified); // Reversed order should NOT match anymore
    }

    #[test]
    fn test_verify_note_cmu_mismatch() {
        let sk = test_sk();
        let (addr, _) = keys::derive_address(&sk, 0).unwrap();
        let mut diversifier = [0u8; 11];
        diversifier.copy_from_slice(&addr[0..11]);
        let rcm = [0u8; 32];

        let wrong_cmu = [0xAB; 32];
        let verified = verify_note_cmu(&sk, &diversifier, 100_000, &rcm, &wrong_cmu, false).unwrap();
        assert!(!verified);
    }

    #[test]
    fn test_try_decrypt_note_invalid_input() {
        // Should return None for garbage input
        let ivk = [0u8; 32];
        let epk = [0u8; 32];
        let cmu = [0u8; 32];
        let ciphertext = [0u8; 580];

        let result = try_decrypt_note(&ivk, &epk, &cmu, &ciphertext);
        assert!(result.is_none());
    }

    #[test]
    fn test_parallel_decrypt_empty() {
        let sk = test_sk();
        let result = try_decrypt_notes_parallel(&sk, &[], 500_000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_invalid_sk_length() {
        let result = compute_nullifier(&[0u8; 16], &[0u8; 11], 100, &[0u8; 32], 0, false);
        assert!(matches!(result, Err(CryptoError::InvalidSpendingKey)));
    }
}
