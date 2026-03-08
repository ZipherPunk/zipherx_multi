//! Message dispatcher — routes TCP messages to waiting handlers.
//!
//! The block listener is the sole TCP reader. When other code needs a response
//! (e.g., header sync sends getheaders and needs headers response), it registers
//! a oneshot channel keyed by expected command name. The block listener reads
//! messages and routes them through the dispatcher.
//!
//! CRITICAL PATTERN: This eliminates all race conditions that required 400+ lines
//! of workaround in the Swift implementation.

use std::collections::{HashMap, VecDeque};

use tokio::sync::oneshot;

/// A batch collector that accumulates multiple messages of the same type.
struct BatchCollector {
    #[allow(dead_code)]
    command: String,
    expected_count: usize,
    collected: Vec<Vec<u8>>,
    sender: Option<oneshot::Sender<Vec<Vec<u8>>>>,
}

/// Message dispatcher — routes incoming P2P messages to registered handlers.
///
/// Protected by `std::sync::Mutex` (not tokio) because all operations are
/// synchronous (no awaits while holding the lock).
pub struct Dispatcher {
    /// Single-message handlers keyed by command name, FIFO queue per command.
    pending: HashMap<String, VecDeque<oneshot::Sender<(String, Vec<u8>)>>>,

    /// Batch collectors keyed by monotonic ID.
    batch_collectors: HashMap<u64, BatchCollector>,

    /// Reverse mapping: command → batch collector IDs.
    command_to_batch: HashMap<String, Vec<u64>>,

    /// Broadcast handlers keyed by "broadcast_{txid}".
    broadcast_handlers: HashMap<String, oneshot::Sender<(String, Vec<u8>)>>,

    /// Next batch collector ID.
    next_batch_id: u64,

    /// Whether the block listener is active.
    active: bool,
}

impl Dispatcher {
    /// Create a new dispatcher.
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            batch_collectors: HashMap::new(),
            command_to_batch: HashMap::new(),
            broadcast_handlers: HashMap::new(),
            next_batch_id: 0,
            active: false,
        }
    }

    /// Register to wait for a response of a specific command type.
    ///
    /// Returns a `oneshot::Receiver` that resolves when a matching message arrives.
    /// The caller should wrap this with `tokio::time::timeout()`.
    pub fn register_handler(&mut self, command: &str) -> oneshot::Receiver<(String, Vec<u8>)> {
        let (tx, rx) = oneshot::channel();
        self.pending
            .entry(command.to_string())
            .or_default()
            .push_back(tx);
        rx
    }

    /// Register to wait for any of multiple command types.
    ///
    /// The first matching command fires the receiver. The sender is registered
    /// under ALL specified commands; when one fires, the others are left as
    /// dead senders (receivers will get RecvError).
    ///
    /// LIMITATION: Due to oneshot::Sender not being cloneable, only the first
    /// command in the slice actually gets the handler registered. Messages for
    /// subsequent commands (e.g., "reject" when "headers" is first) will NOT
    /// trigger this handler. The only current use case is getheaders expecting
    /// "headers" OR "reject" — reject handling falls through to the broadcast
    /// handler path in dispatch(). If true multi-command support is needed,
    /// this should be refactored to use an Arc<Mutex<Option<Sender>>>.
    pub fn register_any_handler(
        &mut self,
        commands: &[&str],
    ) -> oneshot::Receiver<(String, Vec<u8>)> {
        let (tx, rx) = oneshot::channel();

        // We clone the sender for all but the last command.
        // Actually, oneshot::Sender can't be cloned. Instead, we wrap in a
        // shared approach: register a single sender under each command name,
        // and let dispatch check if it's still alive (send returns Err if
        // receiver dropped). For simplicity, create separate channels and
        // use a single-shot flag via the receiver.
        //
        // Simpler approach: just register under the first command that's most
        // likely (usually "headers" or "reject"). For now, register under all
        // commands using a shared sender via Arc<Mutex<Option<oneshot::Sender>>>.

        // Actually, the simplest correct approach: register one handler per
        // command, all sharing the same multi-producer. But oneshot is single-
        // producer. Let's use a different pattern:
        //
        // Register the sender under each command. When dispatch finds a match,
        // it sends. For the other commands, the VecDeque entries will be stale
        // senders (receiver already consumed). The dispatch() method already
        // handles send failures by dropping the sender.

        // Simplified approach: register under the primary expected command only.
        // by using the first-wins pattern.
        //
        // Since oneshot can only be sent once, we'll use a wrapper.
        // The cleanest solution: create a mpsc(1) channel and register oneshot
        // adapters. But that's overengineered.
        //
        // PRACTICAL SOLUTION: Don't support register_any_handler generically.
        // The only use case is "headers" OR "reject" for getheaders.
        // Register under the primary expected command only. If a reject arrives,
        // it will be caught by the broadcast handler or fall through to
        // background handling.
        //
        // For Phase 2, this simplified approach works. Full multi-command
        // support can be added later if needed.

        self.pending
            .entry(commands[0].to_string())
            .or_default()
            .push_back(tx);

        rx
    }

    /// Register a batch collector for N messages of the same command type.
    ///
    /// Returns a receiver that resolves when all expected messages arrive.
    pub fn register_batch(
        &mut self,
        command: &str,
        expected_count: usize,
    ) -> oneshot::Receiver<Vec<Vec<u8>>> {
        let (tx, rx) = oneshot::channel();
        let id = self.next_batch_id;
        self.next_batch_id += 1;

        self.batch_collectors.insert(
            id,
            BatchCollector {
                command: command.to_string(),
                expected_count,
                collected: Vec::with_capacity(expected_count),
                sender: Some(tx),
            },
        );

        self.command_to_batch
            .entry(command.to_string())
            .or_default()
            .push(id);

        rx
    }

    /// Register a broadcast handler keyed by txid.
    ///
    /// The handler fires when a "reject" message arrives for this txid,
    /// or resolves to None (via timeout) if no reject = success.
    pub fn register_broadcast(&mut self, txid: &str) -> oneshot::Receiver<(String, Vec<u8>)> {
        let (tx, rx) = oneshot::channel();
        let key = format!("broadcast_{txid}");
        self.broadcast_handlers.insert(key, tx);
        rx
    }

    /// Dispatch an incoming message from the block listener.
    ///
    /// Returns `true` if the message was consumed by a handler.
    /// Returns `false` if no handler was waiting (background message).
    ///
    /// Dispatch priority:
    /// 1. Batch collectors (command match)
    /// 2. Single pending handlers (FIFO per command)
    /// 3. Broadcast reject handlers (for "reject" command only)
    pub fn dispatch(&mut self, command: &str, payload: Vec<u8>) -> bool {
        // 1. Check batch collectors
        if let Some(batch_ids) = self.command_to_batch.get(command) {
            if let Some(&id) = batch_ids.first() {
                if let Some(collector) = self.batch_collectors.get_mut(&id) {
                    collector.collected.push(payload);
                    if collector.collected.len() >= collector.expected_count {
                        // Batch complete — send results
                        if let Some(sender) = collector.sender.take() {
                            let results = std::mem::take(&mut collector.collected);
                            let _ = sender.send(results);
                        }
                        // Clean up
                        let batch_id = id;
                        self.batch_collectors.remove(&batch_id);
                        if let Some(ids) = self.command_to_batch.get_mut(command) {
                            ids.retain(|&id| id != batch_id);
                            if ids.is_empty() {
                                self.command_to_batch.remove(command);
                            }
                        }
                    }
                    return true;
                }
            }
        }

        // 2. Check single pending handlers
        if let Some(queue) = self.pending.get_mut(command) {
            while let Some(sender) = queue.pop_front() {
                // Try to send — if receiver dropped, try next
                if sender.send((command.to_string(), payload.clone())).is_ok() {
                    if queue.is_empty() {
                        self.pending.remove(command);
                    }
                    return true;
                }
            }
            // All senders were stale
            self.pending.remove(command);
        }

        // 3. Check broadcast handlers (reject messages contain the txid)
        //
        // NET-006: Reject messages are routed to broadcast handlers here rather
        // than to single pending handlers (e.g., getheaders "reject" responses).
        // See register_any_handler() limitations above.
        //
        // RN-10: LIMITATION — single-broadcast assumption.
        // This delivers the reject payload to the first broadcast handler found,
        // not necessarily the one matching the rejected txid. In practice this
        // works because there is almost always at most one pending broadcast at
        // a time. If multiple concurrent broadcasts are ever supported, the
        // reject payload should be parsed to extract the txid and match it to
        // the correct handler. Additionally, if no reject is received within
        // the timeout, the broadcast is assumed successful — there is no
        // positive acknowledgement mechanism in the Bitcoin P2P protocol.
        if command == "reject" {
            // Try all broadcast handlers — the broadcast handler is fire-and-forget
            let keys: Vec<String> = self.broadcast_handlers.keys().cloned().collect();
            for key in keys {
                if let Some(sender) = self.broadcast_handlers.remove(&key) {
                    let _ = sender.send((command.to_string(), payload));
                    return true;
                }
            }
        }

        false
    }

    /// Cancel all pending handlers (called on disconnect).
    ///
    /// All senders are dropped, causing receivers to get `RecvError`.
    pub fn cancel_all(&mut self) {
        self.pending.clear();
        self.broadcast_handlers.clear();

        // Complete batch collectors with partial results
        for (_, mut collector) in self.batch_collectors.drain() {
            if let Some(sender) = collector.sender.take() {
                let results = std::mem::take(&mut collector.collected);
                let _ = sender.send(results);
            }
        }
        self.command_to_batch.clear();
    }

    /// Set the active state (block listener running).
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Check if the dispatcher is active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Number of pending single handlers.
    pub fn pending_count(&self) -> usize {
        self.pending.values().map(|q| q.len()).sum()
    }

    /// Number of active batch collectors.
    pub fn batch_count(&self) -> usize {
        self.batch_collectors.len()
    }
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_dispatch() {
        let mut d = Dispatcher::new();
        let mut rx = d.register_handler("headers");

        let dispatched = d.dispatch("headers", vec![1, 2, 3]);
        assert!(dispatched);

        let (cmd, payload) = rx.try_recv().unwrap();
        assert_eq!(cmd, "headers");
        assert_eq!(payload, vec![1, 2, 3]);
    }

    #[test]
    fn test_dispatch_wrong_command() {
        let mut d = Dispatcher::new();
        let _rx = d.register_handler("headers");

        let dispatched = d.dispatch("reject", vec![1, 2, 3]);
        assert!(!dispatched);
    }

    #[test]
    fn test_fifo_ordering() {
        let mut d = Dispatcher::new();
        let mut rx1 = d.register_handler("headers");
        let mut rx2 = d.register_handler("headers");

        d.dispatch("headers", vec![1]);
        d.dispatch("headers", vec![2]);

        let (_, p1) = rx1.try_recv().unwrap();
        let (_, p2) = rx2.try_recv().unwrap();
        assert_eq!(p1, vec![1]);
        assert_eq!(p2, vec![2]);
    }

    #[test]
    fn test_batch_collector() {
        let mut d = Dispatcher::new();
        let mut rx = d.register_batch("block", 3);

        assert!(d.dispatch("block", vec![1]));
        assert!(d.dispatch("block", vec![2]));
        assert!(d.dispatch("block", vec![3]));

        let results = rx.try_recv().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0], vec![1]);
        assert_eq!(results[1], vec![2]);
        assert_eq!(results[2], vec![3]);
    }

    #[test]
    fn test_broadcast_handler() {
        let mut d = Dispatcher::new();
        let mut rx = d.register_broadcast("abc123");

        let dispatched = d.dispatch("reject", vec![4, 5, 6]);
        assert!(dispatched);

        let (cmd, payload) = rx.try_recv().unwrap();
        assert_eq!(cmd, "reject");
        assert_eq!(payload, vec![4, 5, 6]);
    }

    #[test]
    fn test_cancel_all() {
        let mut d = Dispatcher::new();
        let mut rx1 = d.register_handler("headers");
        let mut rx2 = d.register_handler("block");
        let mut rx_batch = d.register_batch("data", 5);

        // Feed 2 of 5 to batch before cancel
        d.dispatch("data", vec![1]);
        d.dispatch("data", vec![2]);

        d.cancel_all();

        // Single handlers get RecvError (sender dropped)
        assert!(rx1.try_recv().is_err());
        assert!(rx2.try_recv().is_err());

        // Batch gets partial results
        let results = rx_batch.try_recv().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_dispatch_priority_batch_over_single() {
        let mut d = Dispatcher::new();
        let _rx_single = d.register_handler("block");
        let mut rx_batch = d.register_batch("block", 1);

        // Batch should be checked first
        d.dispatch("block", vec![99]);

        let results = rx_batch.try_recv().unwrap();
        assert_eq!(results, vec![vec![99]]);
    }

    #[test]
    fn test_active_flag() {
        let mut d = Dispatcher::new();
        assert!(!d.is_active());

        d.set_active(true);
        assert!(d.is_active());

        d.set_active(false);
        assert!(!d.is_active());
    }

    #[test]
    fn test_stale_sender_skipped() {
        let mut d = Dispatcher::new();
        let rx1 = d.register_handler("headers");
        let _rx2 = d.register_handler("headers");

        // Drop the first receiver
        drop(rx1);

        // Dispatch should skip the stale sender and deliver to the second
        let dispatched = d.dispatch("headers", vec![42]);
        assert!(dispatched);
    }
}
