//! A read-only git cache for remote stores.
//!
//! The cache holds one bare clone for each URL. Documents are read from
//! git objects at the revision that each consumer declares. A bare clone
//! has no working tree, so two consumers that pin different revisions of
//! one URL share the same fetch and cannot overwrite each other. A
//! working tree would give the last consumer that synchronized control
//! of what every other consumer reads.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// The directory that holds the bare clones.
pub fn cache_root() -> PathBuf {
    if let Ok(dir) = std::env::var("MDSTORE_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".cache")
        });
    base.join("mdstore").join("stores")
}

/// The slot name for one URL: a readable part and a hash, so two URLs
/// with the same last segment stay separate.
pub fn slot_name(url: &str) -> String {
    let canonical = super::store::canonical_url_for_cache(url);
    let readable: String = canonical
        .rsplit('/')
        .next()
        .unwrap_or("store")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    format!("{readable}-{}", short_hash(&canonical))
}

/// The cache directory for one URL.
pub fn cache_dir(url: &str) -> PathBuf {
    cache_root().join(slot_name(url))
}

/// A stable short hash. FNV-1a: this names a directory, so it needs to
/// be stable and short, not cryptographic.
fn short_hash(s: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn git(args: &[&str], dir: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.arg("-C").arg(d);
    }
    cmd.args(args);
    // A prompt would block a command that the user expects to finish.
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    let out = cmd
        .output()
        .map_err(|e| Error::InvalidStore(format!("cannot run git: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(Error::InvalidStore(format!(
            "git {}: {stderr}",
            args.join(" ")
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// True when a bare clone for this URL is present.
pub fn is_cached(url: &str) -> bool {
    cache_dir(url).join("HEAD").exists()
}

/// Make the bare clone if it is absent. This is the only operation that
/// reaches the network without an explicit `sync`.
pub fn ensure_clone(url: &str) -> Result<PathBuf> {
    let dir = cache_dir(url);
    if dir.join("HEAD").exists() {
        ensure_slot_matches(&dir, url)?;
        return Ok(dir);
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    git(
        &["clone", "--bare", "--quiet", url, &dir.to_string_lossy()],
        None,
    )?;
    Ok(dir)
}

/// Refuse a slot that holds a different repository.
///
/// The slot name comes from a canonicalization that treats https, scp,
/// and a `.git` suffix as one repository. Two URLs that are not the
/// same repository can still land in one slot, and serving the wrong
/// content silently is worse than failing.
fn ensure_slot_matches(dir: &Path, url: &str) -> Result<()> {
    let Some(existing) = origin_url(dir) else {
        return Ok(());
    };
    let want = super::store::canonical_url_for_cache(url);
    let have = super::store::canonical_url_for_cache(&existing);
    if want == have {
        return Ok(());
    }
    Err(Error::InvalidStore(format!(
        "the cache slot for {url} already holds {existing}; remove {} to re-fetch",
        dir.display()
    )))
}

/// Fetch the current state of every branch.
pub fn fetch(url: &str) -> Result<()> {
    let dir = ensure_clone(url)?;
    git(
        &[
            "fetch",
            "--quiet",
            "--prune",
            "origin",
            "+refs/heads/*:refs/heads/*",
        ],
        Some(&dir),
    )?;
    Ok(())
}

/// The commit that a declared revision names. `None` takes the head of
/// the default branch, which only an explicit fetch advances.
pub fn resolve_rev(dir: &Path, rev: Option<&str>) -> Result<String> {
    let target = rev.unwrap_or("HEAD");
    let out = git(&["rev-parse", target], Some(dir))?;
    Ok(out.trim().to_string())
}

/// The files under a directory prefix at one revision.
pub fn list_tree(dir: &Path, rev: &str, prefix: &str) -> Result<Vec<String>> {
    let spec = if prefix.is_empty() {
        rev.to_string()
    } else {
        format!("{rev}:{prefix}")
    };
    // A missing directory at that revision is an empty store, not an
    // error: a store can add its notes later than the revision pinned.
    let out = match git(&["ls-tree", "-r", "--name-only", &spec], Some(dir)) {
        Ok(out) => out,
        Err(_) => return Ok(Vec::new()),
    };
    Ok(out.lines().map(|l| l.to_string()).collect())
}

/// The content of one file at one revision.
pub fn show(dir: &Path, rev: &str, path: &str) -> Result<String> {
    git(&["show", &format!("{rev}:{path}")], Some(dir))
}

/// How long ago the cache last fetched, in seconds. A stale answer must
/// be visibly stale, so consumers print this next to remote content.
pub fn seconds_since_fetch(dir: &Path) -> Option<u64> {
    let head = dir.join("FETCH_HEAD");
    let path = if head.exists() {
        head
    } else {
        dir.join("HEAD")
    };
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    modified.elapsed().ok().map(|d| d.as_secs())
}

/// A duration for people: "3d", "5h", "12m", "just now".
pub fn humanize_age(seconds: u64) -> String {
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86_399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

/// The origin remote of a checkout, for store identity.
pub fn origin_url(path: &Path) -> Option<String> {
    if !path.join(".git").exists() && !path.join("HEAD").exists() {
        return None;
    }
    git(&["remote", "get-url", "origin"], Some(path))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_directories_are_stable_and_distinct() {
        let a = cache_dir("https://example.com/org/kb");
        let b = cache_dir("git@example.com:org/kb.git");
        let c = cache_dir("https://example.com/other/kb");
        assert_eq!(a, b, "one repository, one cache");
        assert_ne!(a, c, "same last segment, different repository");
        assert!(
            a.file_name().unwrap().to_string_lossy().starts_with("kb-"),
            "the name stays readable"
        );
    }

    #[test]
    fn a_slot_holding_another_repository_is_refused() {
        // Two URLs that are not the same repository can land in one
        // slot, because the slot name merges https, scp, and .git.
        let base = std::env::temp_dir().join(format!(
            "mdstore-slot-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        git(&["init", "--quiet", "-b", "main"], Some(&base)).unwrap();
        git(
            &["remote", "add", "origin", "https://example.com/org/other"],
            Some(&base),
        )
        .unwrap();

        let err = ensure_slot_matches(&base, "https://example.com/org/kb")
            .unwrap_err()
            .to_string();
        assert!(err.contains("already holds"), "{err}");

        // The same repository written another way is still a match.
        assert!(ensure_slot_matches(&base, "git@example.com:org/other.git").is_ok());
    }

    #[test]
    fn ages_read_as_english() {
        assert_eq!(humanize_age(0), "just now");
        assert_eq!(humanize_age(59), "just now");
        assert_eq!(humanize_age(60), "1m ago");
        assert_eq!(humanize_age(7200), "2h ago");
        assert_eq!(humanize_age(200_000), "2d ago");
    }

    #[test]
    fn a_real_repository_round_trips_through_the_cache() {
        // Build a repository, clone it into the cache, and read a file
        // from git objects at a pinned revision.
        let base = std::env::temp_dir().join(format!("mdstore-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let origin = base.join("origin");
        std::fs::create_dir_all(origin.join("notes")).unwrap();
        std::fs::write(origin.join("notes/a1-first.md"), "---\ntitle: First\n---\n").unwrap();
        for args in [
            vec!["init", "--quiet", "-b", "main"],
            vec!["add", "-A"],
            vec![
                "-c",
                "user.email=t@e",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "one",
            ],
        ] {
            git(&args, Some(&origin)).unwrap();
        }
        let first = git(&["rev-parse", "HEAD"], Some(&origin))
            .unwrap()
            .trim()
            .to_string();

        // A second commit that the pinned revision must not see.
        std::fs::write(
            origin.join("notes/b2-second.md"),
            "---\ntitle: Second\n---\n",
        )
        .unwrap();
        git(&["add", "-A"], Some(&origin)).unwrap();
        git(
            &[
                "-c",
                "user.email=t@e",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "two",
            ],
            Some(&origin),
        )
        .unwrap();

        let url = origin.to_string_lossy().to_string();
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };
        let dir = ensure_clone(&url).unwrap();
        assert!(is_cached(&url));

        let head = resolve_rev(&dir, None).unwrap();
        assert_eq!(list_tree(&dir, &head, "notes").unwrap().len(), 2);

        // The pin sees only the first commit, and both revisions read
        // out of the same clone.
        assert_eq!(
            list_tree(&dir, &first, "notes").unwrap(),
            vec!["a1-first.md"]
        );
        let text = show(&dir, &first, "notes/a1-first.md").unwrap();
        assert!(text.contains("title: First"));

        assert!(seconds_since_fetch(&dir).is_some());
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }
}
