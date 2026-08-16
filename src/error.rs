#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing frontmatter delimiter")]
    MissingFrontmatter,

    #[error("missing closing frontmatter delimiter")]
    UnclosedFrontmatter,

    #[error("{0}")]
    Yaml(#[from] yaml_serde::Error),

    #[error("invalid provenance: {0}")]
    InvalidProvenance(String),

    #[error("invalid store configuration: {0}")]
    InvalidStore(String),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error(
        "store config format {found} is newer than this build supports ({supported}) — upgrade the tool"
    )]
    UnsupportedFormat { found: u32, supported: u32 },
}

pub type Result<T> = std::result::Result<T, Error>;
