pub mod document;
pub mod error;
pub mod slug;

pub use document::Document;
pub use error::{Error, Result};
pub use slug::{extract_prefix, generate_prefix, has_prefix, slugify};
