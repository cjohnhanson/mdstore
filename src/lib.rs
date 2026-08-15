pub mod blob;
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
pub use provenance::{Marker, Span};
pub use snapshot::{DocId, DocumentSource, Snapshot};
pub use registry::Registry;
pub use store::{
    Member, StoreContent, StoreGraph, StoreId, StoreRef, StoreSource, StoresConfig, is_plain_stem,
};
pub use selector::Selector;
pub use slug::{extract_prefix, generate_prefix, has_prefix, slugify};
