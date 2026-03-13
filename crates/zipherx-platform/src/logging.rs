//! Platform logging trait.

/// Log routing to platform-specific logger (os_log, logcat, Event Log, etc.).
pub trait PlatformLogger: Send + Sync {
    /// Log a message at the given level.
    fn log(&self, level: LogLevel, message: &str);
}

/// Log severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}
