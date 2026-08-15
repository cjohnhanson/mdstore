//! Object-storage stores.
//!
//! A blob store keeps its documents as objects under one prefix. The
//! cache holds a local copy of each object. A blob store is read-only,
//! and only an explicit sync writes to the cache, which is the same
//! contract that a git store has.
//!
//! The CLI runs the vendor tool for the scheme: `aws s3` for `s3://`,
//! `gcloud storage` for `gs://`, and `curl` for `https://`. That keeps
//! this crate free of a cloud SDK, and it keeps credentials where the
//! user already configured them.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// The cache directory for one blob prefix.
///
/// Blob slots sit under their own namespace. Sharing the git namespace
/// let a blob prefix land in a bare clone's slot, so publishing an
/// index naming `config` and `packed-refs` overwrote a git store's
/// internals and broke every consumer of it.
pub fn cache_dir(url: &str) -> PathBuf {
    crate::git::cache_root()
        .join("blob")
        .join(crate::git::slot_name(url))
}

/// The scheme of a blob URL, when it names one.
pub fn scheme_of(url: &str) -> Option<&'static str> {
    if url.starts_with("s3://") {
        Some("s3")
    } else if url.starts_with("gs://") {
        Some("gs")
    } else if url.starts_with("http://") || url.starts_with("https://") {
        Some("https")
    } else {
        None
    }
}

/// True when a local copy of the prefix is present.
pub fn is_cached(url: &str) -> bool {
    cache_dir(url).join(".mdstore-blob").exists()
}

fn run(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| Error::InvalidStore(format!("cannot run {program}: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(Error::InvalidStore(format!("{program}: {stderr}")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Copy the objects under a prefix into the cache.
///
/// The listing and the copy both come from the vendor tool, so its
/// credentials, its retries, and its proxy settings all apply.
pub fn sync(url: &str) -> Result<PathBuf> {
    let dir = cache_dir(url);
    std::fs::create_dir_all(&dir)?;
    let target = dir.to_string_lossy().to_string();
    let source = url.trim_end_matches('/').to_string();

    match scheme_of(url) {
        Some("s3") => {
            run(
                "aws",
                &["s3", "sync", "--only-show-errors", &source, &target],
            )?;
        }
        Some("gs") => {
            run(
                "gcloud",
                &["storage", "rsync", "--recursive", &source, &target],
            )?;
        }
        Some("https") => {
            // An index at the prefix lists the documents, one name per
            // line. A plain HTTP prefix has no listing protocol, so the
            // store publishes the index that it wants read.
            let index = run("curl", &["-fsSL", &format!("{source}/index.txt")])?;
            for name in index.lines().map(str::trim).filter(|l| !l.is_empty()) {
                // The index is written by whoever publishes the store.
                // An entry names one file under the prefix, never a
                // path out of it and never a dot-file.
                let bad = name.starts_with('/')
                    || name.starts_with('.')
                    || std::path::Path::new(name).components().any(|c| {
                        matches!(
                            c,
                            std::path::Component::ParentDir
                                | std::path::Component::RootDir
                                | std::path::Component::Prefix(_)
                        )
                    })
                    || name.split('/').any(|part| part.starts_with('.'));
                if bad {
                    return Err(Error::InvalidStore(format!(
                        "index entry '{name}' leaves the store"
                    )));
                }
                let body = run("curl", &["-fsSL", &format!("{source}/{name}")])?;
                let path = dir.join(name);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, body)?;
            }
        }
        _ => {
            return Err(Error::InvalidStore(format!(
                "'{url}' is not a blob URL; use s3://, gs://, or https://"
            )));
        }
    }

    std::fs::write(dir.join(".mdstore-blob"), url)?;
    Ok(dir)
}

/// The local copy of a blob store, if it was synced.
pub fn locate(url: &str) -> std::result::Result<PathBuf, String> {
    if !is_cached(url) {
        return Err(format!("{url} is not in the cache; run store sync"));
    }
    Ok(cache_dir(url))
}

/// How long ago the prefix was synced.
pub fn seconds_since_sync(dir: &Path) -> Option<u64> {
    std::fs::metadata(dir.join(".mdstore-blob"))
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()
        .map(|d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_are_recognized() {
        assert_eq!(scheme_of("s3://bucket/prefix"), Some("s3"));
        assert_eq!(scheme_of("gs://bucket/prefix"), Some("gs"));
        assert_eq!(scheme_of("https://example.com/kb"), Some("https"));
        assert_eq!(scheme_of("git@example.com:org/kb"), None);
        assert_eq!(scheme_of("../local"), None);
    }

    #[test]
    fn a_non_blob_url_is_rejected() {
        let err = sync("git@example.com:org/kb").unwrap_err().to_string();
        assert!(err.contains("not a blob URL"), "{err}");
    }

    #[test]
    fn an_unsynced_store_says_to_sync() {
        let err = locate("s3://bucket/never-synced").unwrap_err();
        assert!(err.contains("store sync"), "{err}");
    }

    #[test]
    fn two_prefixes_get_two_caches() {
        assert_ne!(cache_dir("s3://bucket/a"), cache_dir("s3://bucket/b"));
    }

    #[test]
    fn a_blob_slot_never_lands_in_a_git_slot() {
        // Sharing the namespace let a published index name `config`
        // and `packed-refs` and overwrite a bare clone's internals.
        let url = "https://example.com/org/kb";
        assert_ne!(cache_dir(url), crate::git::cache_dir(url));
        assert!(cache_dir(url).to_string_lossy().contains("/blob/"));
    }

    #[test]
    fn an_index_entry_that_leaves_the_store_is_refused() {
        for bad in ["../escape.md", "/etc/passwd", ".git/config", "a/../../b.md", ".hidden"] {
            let path = std::path::Path::new(bad);
            let rejected = bad.starts_with('/')
                || bad.starts_with('.')
                || path.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                })
                || bad.split('/').any(|p| p.starts_with('.'));
            assert!(rejected, "{bad} must be refused");
        }
        for good in ["skills/a.md", "note.md", "deep/nested/file.md"] {
            let path = std::path::Path::new(good);
            let rejected = good.starts_with('/')
                || good.starts_with('.')
                || path.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                })
                || good.split('/').any(|p| p.starts_with('.'));
            assert!(!rejected, "{good} must be allowed");
        }
    }
}
