//! Operating-system integration boundaries.
//!
//! Platform-specific macOS and Windows implementations will live behind shared
//! interfaces in this module.

mod filesystem;

pub(crate) use filesystem::{
    create_private_file, ensure_private_directory, replace_file_atomically, sync_directory,
};
