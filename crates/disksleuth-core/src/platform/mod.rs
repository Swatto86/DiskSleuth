/// Platform-specific functionality — Windows drive enumeration,
/// permission checks, and system utilities.
pub mod drives;
pub mod permissions;
pub mod shell;

pub use drives::{enumerate_drives, DriveInfo, DriveType};
pub use permissions::is_elevated;
pub use shell::move_to_recycle_bin;
