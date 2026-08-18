#![forbid(unsafe_code)]

mod diagnostics;
mod filesystem;
mod lease;
mod manifest;
mod operations;
mod store;

pub use lease::{ServiceLease, acquire_service_lease};
pub use operations::{create_backup, restore_backup, verify_backup};

pub(crate) const MAXIMUM_FILES: usize = 20_000;
pub(crate) const MAXIMUM_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAXIMUM_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
pub(crate) const MAXIMUM_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
