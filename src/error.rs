#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing frontmatter delimiter")]
    MissingFrontmatter,

    #[error("missing closing frontmatter delimiter")]
    UnclosedFrontmatter,

    #[error("{0}")]
    Yaml(#[from] serde_yml::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
