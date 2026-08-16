pub mod blob;
pub mod confined;
pub mod document;
pub mod error;
pub mod git;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod provenance;
pub mod registry;
pub mod selector;
pub mod slug;
pub mod snapshot;
pub mod store;

pub use document::{Document, parse, serialize};
pub use error::{Error, Result};
pub use provenance::{Marker, Span, markers_in};
pub use registry::Registry;
pub use selector::Selector;
pub use slug::{extract_prefix, generate_prefix, has_prefix, slugify};
pub use snapshot::{DocId, DocumentSource, Snapshot};
pub use store::{
    Member, StoreContent, StoreGraph, StoreId, StoreRef, StoreSource, StoresConfig, is_plain_stem,
    member_identity,
};

/// Tests that set `MDSTORE_CACHE_DIR` take this lock first. The env is
/// process-global and cargo runs tests on threads.
#[cfg(test)]
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
