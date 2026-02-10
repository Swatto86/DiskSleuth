/// Platform-specific functionality — Windows drive enumeration,
/// permission checks, and system utilities.

pub mod drives;
pub mod permissions;

pub use drives::{enumerate_drives, DriveInfo, DriveType};
pub use permissions::is_elevated;
