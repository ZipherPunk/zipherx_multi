//! Clipboard access trait.

use crate::PlatformError;

/// System clipboard for copy/paste operations.
pub trait Clipboard: Send + Sync {
    /// Copy text to the system clipboard.
    fn copy_text(&self, text: &str) -> Result<(), PlatformError>;

    /// Read text from the system clipboard.
    fn paste_text(&self) -> Result<Option<String>, PlatformError>;
}
