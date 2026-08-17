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

    /// One operation on a store path failed, with the reason kept.
    ///
    /// The message reads the same as an InvalidStore, so nothing a
    /// person sees changes. The io::Error is kept because a consumer
    /// has to tell a refusal apart from a failure: a directory the
    /// store refuses holds no documents, and a directory it cannot
    /// read is a fault. Flattened to a string, a mode-000 directory
    /// was reported as an empty one.
    #[error("invalid store configuration: {rel} in {root}: {source}")]
    StorePath {
        rel: String,
        root: String,
        source: std::io::Error,
    },

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error(
        "store config format {found} is newer than this build supports ({supported}) — upgrade the tool"
    )]
    UnsupportedFormat { found: u32, supported: u32 },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// The reason a store path operation failed, when there is one.
    ///
    /// A consumer uses this to tell a refusal from a fault. A path the
    /// store refuses, or a name that is not a directory, means the
    /// store holds nothing there. A permissions error or an I/O error
    /// means something is wrong and must not read as empty.
    /// True when the store refused the path rather than failing on it.
    ///
    /// A refusal means the store holds nothing there, and a consumer
    /// may treat it as empty. A failure means something is wrong and
    /// must not read as empty.
    ///
    /// cap-std refuses an escaping path with its own message and no
    /// errno, while the operating system supplies one for a real
    /// denial. Both arrive as PermissionDenied, so the kind alone
    /// cannot tell them apart and the errno is what does.
    #[must_use]
    pub fn refused_by_confinement(&self) -> bool {
        match self {
            Error::StorePath { source, .. } => {
                source.raw_os_error().is_none()
                    && source.kind() == std::io::ErrorKind::PermissionDenied
            }
            _ => false,
        }
    }

    #[must_use]
    pub fn io_kind(&self) -> Option<std::io::ErrorKind> {
        match self {
            Error::StorePath { source, .. } => Some(source.kind()),
            Error::Io(e) => Some(e.kind()),
            _ => None,
        }
    }
}
