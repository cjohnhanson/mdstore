pub mod document;
pub mod error;
pub mod selector;
pub mod slug;

pub use document::Document;
pub use error::{Error, Result};
pub use selector::Selector;
pub use slug::{extract_prefix, generate_prefix, has_prefix, slugify};
