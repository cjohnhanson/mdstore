//! Root resolution: which store a command acts on.
//!
//! One rule, identical in every consumer:
//! `--root <dir>` (literal, no walk, no fallback) beats `--home` (the
//! configured root store, named without knowing its path) beats the
//! nearest marker file at or above the cwd beats, for reads only, the
//! configured root store. With nothing left, the error names the fix.
//! No environment variable participates at any tier: an env var is the
//! one input an agent cannot see in the transcript, and a repo can set
//! one through direnv or mise.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::userconfig::UserConfig;

/// Whether the command changes anything. A read that falls back to the
/// root store is announced and harmless. A write that fell back is how
/// a work note lands in the personal repo, so a write never falls
/// back; it fails and names `--home`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Read,
    Write,
}

/// How the root was found. The consumer prints it, so an agent can
/// always tell where a command acted and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// `--root` named it.
    Flag,
    /// `--home` named the configured root store.
    Home,
    /// The nearest marker at or above the cwd.
    Walk,
    /// The configured root store, as a read fallback.
    Config,
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub root: PathBuf,
    pub via: Via,
}

/// The tool-specific words for messages: the marker file name and the
/// noun for a store ("tracker", "store", "library").
#[derive(Debug, Clone, Copy)]
pub struct Vocabulary<'a> {
    pub marker: &'a str,
    pub noun: &'a str,
    pub tool: &'a str,
}

/// Resolve the root for one command.
///
/// `named` is `--root`, `home` is `--home`; the caller enforces that
/// they are mutually exclusive. `cwd` must be absolute.
pub fn resolve_root(
    cwd: &Path,
    named: Option<&Path>,
    home: bool,
    intent: Intent,
    config: &UserConfig,
    vocab: Vocabulary<'_>,
) -> Result<Resolved> {
    let Vocabulary { marker, noun, tool } = vocab;
    if let Some(dir) = named {
        let dir = if dir.is_absolute() {
            dir.to_path_buf()
        } else {
            cwd.join(dir)
        };
        if !has_marker(&dir, marker) {
            return Err(Error::InvalidStore(format!(
                "not a {tool} {noun}: no {marker} in {}",
                dir.display()
            )));
        }
        return Ok(Resolved {
            root: dir,
            via: Via::Flag,
        });
    }
    if home {
        let Some(root) = &config.root_store else {
            return Err(Error::InvalidStore(format!(
                "--home needs a root {noun}; run: {tool} store root <path>"
            )));
        };
        require_root_store(root, vocab)?;
        return Ok(Resolved {
            root: root.clone(),
            via: Via::Home,
        });
    }
    if let Some(found) = walk_up(cwd, marker) {
        return Ok(Resolved {
            root: found,
            via: Via::Walk,
        });
    }
    match (intent, &config.root_store) {
        (Intent::Read, Some(root)) => {
            require_root_store(root, vocab)?;
            Ok(Resolved {
                root: root.clone(),
                via: Via::Config,
            })
        }
        (Intent::Write, Some(root)) => Err(Error::InvalidStore(format!(
            "no {marker} at or above {}; a write does not fall back. Use --home to write \
             to the root {noun} ({}), or --root <dir>.",
            cwd.display(),
            root.display()
        ))),
        (_, None) => Err(Error::InvalidStore(format!(
            "not a {tool} {noun}: no {marker} at or above {}, and no root {noun} is set in \
             ~/.config/{tool}/config.yml (set one: {tool} store root <path>)",
            cwd.display()
        ))),
    }
}

/// A configured root that does not hold the marker is an error, never
/// a silent pass-through: proceeding as unconfigured would turn a typo
/// in the config into behavior that looks like a missing feature.
fn require_root_store(root: &Path, vocab: Vocabulary<'_>) -> Result<()> {
    let Vocabulary { marker, noun, tool } = vocab;
    if !has_marker(root, marker) {
        return Err(Error::InvalidStore(format!(
            "the root {noun} in ~/.config/{tool}/config.yml ({}) has no {marker}; run \
             `{tool} init` there, or set a new path with `{tool} store root <path>`",
            root.display()
        )));
    }
    Ok(())
}

/// The marker must be a regular file. A symlink does not count: a
/// planted link is the cheapest capture.
fn has_marker(dir: &Path, marker: &str) -> bool {
    std::fs::symlink_metadata(dir.join(marker)).is_ok_and(|m| m.is_file())
}

/// Walk from `cwd` to the filesystem root; the first directory holding
/// the marker wins. The walk stops at the first directory the invoking
/// user does not own — git's CVE-2022-24765 boundary — so a marker
/// planted in /tmp or another shared ancestor cannot capture commands.
fn walk_up(cwd: &Path, marker: &str) -> Option<PathBuf> {
    let mut dir = cwd;
    loop {
        if !owned_by_caller(dir) {
            return None;
        }
        if has_marker(dir, marker) {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

#[cfg(unix)]
fn owned_by_caller(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(dir).is_ok_and(|m| m.uid() == nix::unistd::getuid().as_raw())
}

#[cfg(not(unix))]
fn owned_by_caller(_dir: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: Vocabulary<'static> = Vocabulary {
        marker: "tool.yml",
        noun: "tracker",
        tool: "tool",
    };

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mdstore-res-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn cfg(root: Option<&Path>) -> UserConfig {
        UserConfig {
            format: 1,
            root_store: root.map(Path::to_path_buf),
        }
    }

    #[test]
    fn the_nearest_marker_wins_and_the_walk_reports_it() {
        let d = scratch("walk");
        std::fs::write(d.join("tool.yml"), "").unwrap();
        let deep = d.join("src/deep");
        std::fs::create_dir_all(&deep).unwrap();
        let r = resolve_root(&deep, None, false, Intent::Read, &cfg(None), V).unwrap();
        assert_eq!(r.root, d);
        assert_eq!(r.via, Via::Walk);
        // A nested marker shadows the outer one.
        std::fs::write(d.join("src/tool.yml"), "").unwrap();
        let r = resolve_root(&deep, None, false, Intent::Write, &cfg(None), V).unwrap();
        assert_eq!(r.root, d.join("src"));
    }

    #[test]
    fn an_explicit_root_never_walks_and_never_falls_back() {
        let d = scratch("flag");
        std::fs::write(d.join("tool.yml"), "").unwrap();
        let sub = d.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let err = resolve_root(&d, Some(&sub), false, Intent::Read, &cfg(Some(&d)), V)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no tool.yml in"), "{err}");
        let r = resolve_root(&sub, Some(&d), false, Intent::Read, &cfg(None), V).unwrap();
        assert_eq!((r.root, r.via), (d, Via::Flag));
    }

    #[test]
    fn a_symlinked_marker_does_not_count() {
        let d = scratch("link");
        let real = d.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("actual.yml"), "").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(real.join("actual.yml"), d.join("tool.yml")).unwrap();
            assert!(resolve_root(&d, None, false, Intent::Read, &cfg(None), V).is_err());
        }
    }

    #[test]
    fn reads_fall_back_to_the_configured_root_and_writes_do_not() {
        let d = scratch("fall");
        let root = d.join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("tool.yml"), "").unwrap();
        let work = d.join("work");
        std::fs::create_dir_all(&work).unwrap();

        let r = resolve_root(&work, None, false, Intent::Read, &cfg(Some(&root)), V).unwrap();
        assert_eq!((r.root.clone(), r.via), (root.clone(), Via::Config));

        let err = resolve_root(&work, None, false, Intent::Write, &cfg(Some(&root)), V)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--home"), "{err}");
        assert!(err.contains(&root.display().to_string()), "{err}");

        // --home writes to the root from anywhere, including a cwd
        // that has its own store.
        std::fs::write(work.join("tool.yml"), "").unwrap();
        let r = resolve_root(&work, None, true, Intent::Write, &cfg(Some(&root)), V).unwrap();
        assert_eq!((r.root, r.via), (root, Via::Home));
    }

    #[test]
    fn every_terminal_error_names_the_fix() {
        let d = scratch("err");
        let err = resolve_root(&d, None, false, Intent::Read, &cfg(None), V)
            .unwrap_err()
            .to_string();
        assert!(err.contains("store root <path>"), "{err}");
        // The path must name the calling tool's directory. This message
        // is the only place a user learns which file to edit, so a
        // regression to the library's directory sends them to a file
        // nothing reads.
        assert!(err.contains("~/.config/tool/config.yml"), "{err}");
        assert!(!err.contains("config/mdstore"), "{err}");
        let err = resolve_root(&d, None, true, Intent::Read, &cfg(None), V)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--home needs a root"), "{err}");
        // A configured root without the marker is an error, never a
        // silent downgrade to unconfigured.
        let bogus = d.join("bogus");
        std::fs::create_dir_all(&bogus).unwrap();
        let err = resolve_root(&d, None, false, Intent::Read, &cfg(Some(&bogus)), V)
            .unwrap_err()
            .to_string();
        assert!(err.contains("has no tool.yml"), "{err}");
        assert!(err.contains("~/.config/tool/config.yml"), "{err}");
        assert!(!err.contains("config/mdstore"), "{err}");
    }
}
