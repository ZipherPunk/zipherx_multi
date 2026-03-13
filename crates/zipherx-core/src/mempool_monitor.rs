//! Mempool monitor — detects incoming shielded transactions before they are mined.
//!
//! Event-driven via the block listener's dispatcher pattern:
//! - Peer receives "inv" MSG_TX → sends getdata → receives "tx" response
//! - Block listener fires `on_mempool_tx_data` callback with raw TX bytes
//! - This module trial-decrypts Sapling outputs and notifies the UI
//! No separate task, no channel, no extra TCP connection.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use zipherx_network::block_fetcher;
use zipherx_network::broadcast;

use zipherx_crypto::notes;
use zipherx_crypto::types::ENC_CIPHERTEXT_LEN;

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
    on_tx: MempoolTxCallback,
    seen: Mutex<HashSet<Vec<u8>>>,
}

impl MempoolDetector {
    /// Create a new mempool detector.
    ///
    /// `sk_bytes` is the spending key for trial decryption.
    /// `on_tx` is called when a matching transaction is detected.
    pub fn new(sk_bytes: Vec<u8>, on_tx: MempoolTxCallback) -> Arc<Self> {
        Arc::new(Self {
            sk_bytes,
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
    /// Trial-decrypts Sapling outputs and fires the callback if any belong to the wallet.
    fn process_raw_tx(&self, raw_tx: &[u8]) {
        if self.sk_bytes.is_empty() {
            return;
        }

        // Dedup by raw TX bytes (first 64 bytes is enough to identify)
        let dedup_key = if raw_tx.len() >= 64 {
            raw_tx[..64].to_vec()
        } else {
            raw_tx.to_vec()
        };
        {
            let mut seen = self.seen.lock().unwrap();
            if !seen.insert(dedup_key) {
                return;
            }
            // Evict old entries to prevent unbounded growth
            if seen.len() > 1000 {
                seen.clear();
            }
        }

        // Parse the transaction
        let (txid, outputs, _spends) = match block_fetcher::parse_raw_tx(raw_tx) {
            Some(parsed) => parsed,
            None => return,
        };

        if outputs.is_empty() {
            return;
        }

        // Trial-decrypt each Sapling output
        let mut total_value = 0u64;
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
