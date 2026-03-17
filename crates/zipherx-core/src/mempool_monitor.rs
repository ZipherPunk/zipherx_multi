//! Mempool monitor — detects incoming transactions before they are mined.
//!
//! Event-driven via the block listener's dispatcher pattern:
//! - Peer receives "inv" MSG_TX → sends getdata → receives "tx" response
//! - Block listener fires `on_mempool_tx_data` callback with raw TX bytes
//! - This module trial-decrypts Sapling outputs AND matches transparent outputs
//!   against derived addresses, then notifies the UI.
//! No separate task, no channel, no extra TCP connection.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use zipherx_network::block_fetcher;
use zipherx_network::broadcast;

use zipherx_crypto::notes;
use zipherx_crypto::types::ENC_CIPHERTEXT_LEN;

use crate::scanner::TransparentAddressSet;

/// Callback for mempool TX detection.
pub type MempoolTxCallback = Arc<dyn Fn(MempoolTxInfo) + Send + Sync>;

/// Information about a detected mempool transaction.
#[derive(Debug, Clone)]
pub struct MempoolTxInfo {
    /// Transaction ID (display format hex).
    pub txid: String,
    /// Total value of notes belonging to the wallet (zatoshis).
    pub amount: u64,
}

/// Shared state for dedup across multiple callback invocations.
pub struct MempoolDetector {
    sk_bytes: Vec<u8>,
    transparent_addresses: Option<TransparentAddressSet>,
    on_tx: MempoolTxCallback,
    seen: Mutex<HashSet<[u8; 32]>>,
}

impl MempoolDetector {
    /// Create a new mempool detector.
    ///
    /// `sk_bytes` is the spending key for trial decryption.
    /// `on_tx` is called when a matching transaction is detected.
    pub fn new(sk_bytes: Vec<u8>, on_tx: MempoolTxCallback) -> Arc<Self> {
        Arc::new(Self {
            sk_bytes,
            transparent_addresses: None,
            on_tx,
            seen: Mutex::new(HashSet::new()),
        })
    }

    /// Create a new mempool detector with transparent address matching.
    ///
    /// `sk_bytes` is the spending key for Sapling trial decryption.
    /// `seed` is used to derive transparent addresses for matching.
    /// `on_tx` is called when a matching transaction is detected.
    pub fn new_with_transparent(
        sk_bytes: Vec<u8>,
        seed: &[u8],
        on_tx: MempoolTxCallback,
    ) -> Arc<Self> {
        let address_set = TransparentAddressSet::from_seed(seed, 0, 20);
        Arc::new(Self {
            sk_bytes,
            transparent_addresses: Some(address_set),
            on_tx,
            seen: Mutex::new(HashSet::new()),
        })
    }

    /// Build the callback closure for `peer_manager.set_on_mempool_tx_data()`.
    ///
    /// Returns an `Arc<dyn Fn(Vec<u8>) + Send + Sync>` that processes raw TX bytes.
    pub fn into_callback(self: &Arc<Self>) -> Arc<dyn Fn(Vec<u8>) + Send + Sync> {
        let detector = self.clone();
        Arc::new(move |raw_tx: Vec<u8>| {
            detector.process_raw_tx(&raw_tx);
        })
    }

    /// Process a raw transaction received from the network.
    ///
    /// Trial-decrypts Sapling outputs and matches transparent outputs
    /// against derived addresses. Fires the callback if any belong to the wallet.
    fn process_raw_tx(&self, raw_tx: &[u8]) {
        if self.sk_bytes.is_empty() && self.transparent_addresses.is_none() {
            return;
        }

        // Dedup by SHA-256 hash of full raw TX (collision-resistant)
        let tx_hash: [u8; 32] = Sha256::digest(raw_tx).into();
        {
            let mut seen = self.seen.lock().unwrap();
            if !seen.insert(tx_hash) {
                return; // Already processed
            }
            // Evict all entries to prevent unbounded growth (10k threshold
            // is safe with SHA-256 — no collision risk within a session)
            if seen.len() >= 10_000 {
                seen.clear();
            }
        }

        // Parse the transaction including transparent outputs
        let (txid, outputs, _spends, t_outputs) = match block_fetcher::parse_raw_tx_full(raw_tx) {
            Some(parsed) => parsed,
            None => return,
        };

        if outputs.is_empty() && t_outputs.is_empty() {
            return;
        }

        let mut total_value = 0u64;

        // Trial-decrypt each Sapling output
        if !self.sk_bytes.is_empty() {
            for output in &outputs {
                if output.ciphertext.len() < ENC_CIPHERTEXT_LEN {
                    continue;
                }
                if let Some(decrypted) = notes::try_decrypt_note_with_sk(
                    &self.sk_bytes,
                    &output.epk,
                    &output.cmu,
                    &output.ciphertext,
                    0, // mempool — no block height yet
                ) {
                    total_value += decrypted.value;
                }
            }
        }

        // Match transparent outputs against our derived addresses
        if let Some(ref addr_set) = self.transparent_addresses {
            for t_out in &t_outputs {
                if addr_set.match_script(&t_out.script_pubkey).is_some() {
                    total_value += t_out.value;

                    #[cfg(debug_assertions)]
                    {
                        let (addr, is_change, _, _is_imported) =
                            addr_set.match_script(&t_out.script_pubkey).unwrap();
                        eprintln!(
                            "[ZipherX] Mempool: transparent output matched addr={} change={} value={}",
                            addr, is_change, t_out.value
                        );
                    }
                }
            }
        }

        if total_value > 0 {
            // Convert wire-format txid to display format
            let txid_display = broadcast::wire_txid_to_display(&txid);

            #[cfg(debug_assertions)]
            eprintln!(
                "[ZipherX] Mempool: detected incoming TX {} ({} zatoshis)",
                txid_display, total_value
            );

            (self.on_tx)(MempoolTxInfo {
                txid: txid_display,
                amount: total_value,
            });
        }
    }
}
