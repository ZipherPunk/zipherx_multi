//! Local notification trait.

use crate::PlatformError;

/// Local push notifications for transaction events.
pub trait Notifications: Send + Sync {
    /// Send a local notification.
    fn send_notification(&self, title: &str, body: &str) -> Result<(), PlatformError>;

    /// Request notification permissions (no-op on platforms that don't require it).
    fn request_permission(&self) -> Result<bool, PlatformError>;
}
