//! Global tokio runtime manager for ZipherX.
//!
//! The FFI layer (UniFFI) calls sync functions from native threads.
//! This module provides a singleton tokio runtime that bridges sync→async.
//!
//! Pattern:
//! - Native code calls FFI sync function
//! - FFI function calls `block_on(async_operation)` or `spawn(async_operation)`
//! - Async operation runs on the tokio thread pool
//! - CPU-heavy ops (Groth16, storage) use `spawn_blocking`

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::OnceCell;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

use crate::CoreError;

// ============================================================================
// Global State
// ============================================================================

static RUNTIME: OnceCell<Runtime> = OnceCell::new();
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

// ============================================================================
// Public API
// ============================================================================

/// Initialize the global tokio runtime.
///
/// Call once at app startup (before any async operations).
/// Safe to call multiple times — second+ calls are no-ops.
pub fn initialize_runtime() -> Result<(), CoreError> {
    if SHUTDOWN.load(Ordering::SeqCst) {
        return Err(CoreError::RuntimeShutdown);
    }

    // OnceCell guarantees this runs exactly once
    RUNTIME.get_or_try_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .thread_name("zipherx-worker")
            .build()
            .map_err(|e| CoreError::RuntimeError(e.to_string()))
    })?;

    Ok(())
}

/// Get a reference to the global runtime.
///
/// Returns error if not initialized or after shutdown.
pub fn get_runtime() -> Result<&'static Runtime, CoreError> {
    if SHUTDOWN.load(Ordering::SeqCst) {
        return Err(CoreError::RuntimeShutdown);
    }
    RUNTIME.get().ok_or(CoreError::RuntimeNotInitialized)
}

/// Check if the runtime is initialized and not shut down.
pub fn is_runtime_ready() -> bool {
    RUNTIME.get().is_some() && !SHUTDOWN.load(Ordering::SeqCst)
}

/// Run an async future to completion on the global runtime.
///
/// Blocks the calling thread until the future completes.
/// Use this from FFI sync functions to bridge to async.
pub fn block_on<F>(future: F) -> Result<F::Output, CoreError>
where
    F: Future,
{
    let rt = get_runtime()?;
    Ok(rt.block_on(future))
}

/// Spawn an async task on the global runtime.
///
/// Returns a JoinHandle that can be awaited for the result.
/// Use for fire-and-forget async operations (background sync, etc.).
pub fn spawn<F>(future: F) -> Result<JoinHandle<F::Output>, CoreError>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let rt = get_runtime()?;
    Ok(rt.spawn(future))
}

/// Spawn a blocking operation on a dedicated thread pool.
///
/// Use for CPU-heavy (Groth16 proofs) or blocking I/O (rusqlite) operations
/// that would block the async event loop.
pub fn spawn_blocking_on_runtime<F, R>(f: F) -> Result<JoinHandle<R>, CoreError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let rt = get_runtime()?;
    Ok(rt.spawn(async {
        tokio::task::spawn_blocking(f)
            .await
            .expect("spawn_blocking task panicked")
    }))
}

/// Signal that the runtime should not accept new work.
///
/// Does NOT drop the runtime (OnceCell can't be cleared).
/// Existing tasks will complete but new spawn/block_on calls will fail.
pub fn shutdown_runtime() {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_init_and_ready() {
        initialize_runtime().unwrap();
        assert!(is_runtime_ready());
        assert!(get_runtime().is_ok());
    }

    #[test]
    fn test_double_init_is_ok() {
        initialize_runtime().unwrap();
        // Second init is a no-op, not an error
        initialize_runtime().unwrap();
        assert!(is_runtime_ready());
    }

    #[test]
    fn test_block_on_runs_future() {
        initialize_runtime().unwrap();
        let result = block_on(async { 42 }).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_spawn_returns_handle() {
        initialize_runtime().unwrap();
        let rt = get_runtime().unwrap();
        let handle = spawn(async { "hello" }).unwrap();
        let result = rt.block_on(handle).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_spawn_blocking_on_runtime_runs() {
        initialize_runtime().unwrap();
        let rt = get_runtime().unwrap();
        let handle = spawn_blocking_on_runtime(|| {
            // Simulate CPU-heavy work
            std::thread::sleep(std::time::Duration::from_millis(10));
            99
        })
        .unwrap();
        let result = rt.block_on(handle).unwrap();
        assert_eq!(result, 99);
    }
}
