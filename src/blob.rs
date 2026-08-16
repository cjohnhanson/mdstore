//! Object-storage stores, over https.
//!
//! A blob store keeps its documents as objects under one prefix. The
//! cache holds a local copy of each object. A blob store is read-only,
//! and only an explicit sync writes to the cache, which is the same
//! contract that a git store has.
//!
//! The one scheme is `https://` (and `http://`): the prefix publishes
//! an `index.txt` that names its documents, one per line, and each is
//! fetched by GET. Nothing here spawns a process. `s3://` and `gs://`
//! were once synced by the vendor CLIs; those schemes are refused now,
//! with the alternative in the message.

use std::path::{Path, PathBuf};

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

/// True when a listed name stays inside the store: no leading `/`, no
/// empty or dot-led component, no parent, root, or prefix component.
pub(crate) fn name_stays_inside(name: &str) -> bool {
    !(name.is_empty()
        || name.starts_with('/')
        || Path::new(name).components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
        || name
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.')))
}

/// Copy the objects under a prefix into the cache.
///
/// The fetch fills a directory beside the slot and renames it into
/// place, so a retracted document does not survive in the cache and an
/// interrupted sync leaves the previous copy untouched.
pub fn sync(url: &str) -> Result<PathBuf> {
    match scheme_of(url) {
        Some("https") => {}
        Some(scheme) => {
            return Err(Error::InvalidStore(format!(
                "'{url}': {scheme}:// stores are not supported; mdstore spawns no vendor CLI. \
                 Publish the prefix over https with an index.txt, or declare a git store."
            )));
        }
        None => {
            return Err(Error::InvalidStore(format!(
                "'{url}' is not a blob URL; use https://"
            )));
        }
    }
    let dir = cache_dir(url);
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let source = url.trim_end_matches('/').to_string();
    let staging = dir.with_extension(format!("new-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let result = fill(&source, &staging);
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    std::fs::write(staging.join(".mdstore-blob"), url)?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&staging, &dir)?;
    Ok(dir)
}

fn fill(source: &str, into: &Path) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("mdstore/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| Error::InvalidStore(format!("http client: {e}")))?;
    let get = |what: &str| -> Result<Vec<u8>> {
        let resp = client
            .get(what)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|e| Error::InvalidStore(format!("GET {what}: {e}")))?;
        let bytes = resp
            .bytes()
            .map_err(|e| Error::InvalidStore(format!("GET {what}: {e}")))?;
        Ok(bytes.to_vec())
    };
    // The index is written by whoever publishes the store. An entry
    // names one file under the prefix, never a path out of it and never
    // a dot-file.
    let index = String::from_utf8_lossy(&get(&format!("{source}/index.txt"))?).into_owned();
    for name in index.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !name_stays_inside(name) {
            return Err(Error::InvalidStore(format!(
                "index entry '{name}' leaves the store"
            )));
        }
        let body = get(&format!("{source}/{name}"))?;
        let path = into.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, body)?;
    }
    Ok(())
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
        for bad in [
            "../escape.md",
            "/etc/passwd",
            ".git/config",
            "a/../../b.md",
            ".hidden",
            "a//b.md",
            "dir/",
            "",
        ] {
            assert!(!name_stays_inside(bad), "{bad:?} must be refused");
        }
        for good in ["skills/a.md", "note.md", "deep/nested/file.md"] {
            assert!(name_stays_inside(good), "{good} must be allowed");
        }
    }

    #[test]
    fn a_vendor_scheme_is_refused_with_the_alternative() {
        for url in ["s3://bucket/prefix", "gs://bucket/prefix"] {
            let err = sync(url).unwrap_err().to_string();
            assert!(err.contains("https"), "{err}");
            assert!(err.contains("git store"), "{err}");
        }
    }

    /// A loopback HTTP server that serves a fixed set of paths, so the
    /// https sync is exercised end to end with no network.
    type Routes = std::sync::Arc<std::sync::Mutex<Vec<(&'static str, &'static str)>>>;

    fn serve(routes: Vec<(&'static str, &'static str)>) -> (String, Routes) {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let routes: Routes = std::sync::Arc::new(std::sync::Mutex::new(routes));
        let served = routes.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let body = served
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(p, _)| *p == path)
                    .map(|(_, b)| *b);
                let response = match body {
                    Some(b) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
                        b.len()
                    ),
                    None => {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_string()
                    }
                };
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://{addr}/kb"), routes)
    }

    #[test]
    fn an_https_store_syncs_its_index_and_documents_into_the_cache() {
        let _env = crate::env_lock();
        let base = std::env::temp_dir().join(format!("mdstore-blob-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", &base) };
        let (url, routes) = serve(vec![
            ("/kb/index.txt", "a.md\nnotes/b.md\n"),
            ("/kb/a.md", "---\ntitle: A\n---\n"),
            ("/kb/notes/b.md", "---\ntitle: B\n---\n"),
        ]);
        let dir = sync(&url).unwrap();
        assert!(is_cached(&url));
        assert_eq!(
            std::fs::read_to_string(dir.join("a.md")).unwrap(),
            "---\ntitle: A\n---\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("notes/b.md")).unwrap(),
            "---\ntitle: B\n---\n"
        );
        assert_eq!(locate(&url).unwrap(), dir);
        assert!(seconds_since_sync(&dir).is_some());

        // The publisher retracts b.md and changes a.md. The next sync of
        // the same URL follows: b.md leaves the cache, a.md updates.
        *routes.lock().unwrap() = vec![("/kb/index.txt", "a.md\n"), ("/kb/a.md", "v2\n")];
        let dir2 = sync(&url).unwrap();
        assert_eq!(dir2, dir);
        assert_eq!(std::fs::read_to_string(dir.join("a.md")).unwrap(), "v2\n");
        assert!(
            !dir.join("notes").exists(),
            "a retracted document leaves the cache"
        );

        // An index that names a document the server lacks fails the
        // sync, and the previous copy stays whole.
        *routes.lock().unwrap() = vec![("/kb/index.txt", "missing.md\n")];
        let err = sync(&url).unwrap_err().to_string();
        assert!(err.contains("404"), "{err}");
        assert_eq!(std::fs::read_to_string(dir.join("a.md")).unwrap(), "v2\n");

        // An escaping index entry is refused before any fetch.
        *routes.lock().unwrap() = vec![("/kb/index.txt", "../escape.md\n")];
        let err = sync(&url).unwrap_err().to_string();
        assert!(err.contains("leaves the store"), "{err}");

        // A URL that never synced is not in the cache.
        let (other, _) = serve(vec![]);
        assert!(!is_cached(&other));
        assert!(locate(&other).unwrap_err().contains("store sync"));
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }
}
