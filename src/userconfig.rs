//! The user-level config: `~/.config/mdstore/config.yml`.
//!
//! One file, shared by every consumer, so the rule an agent must
//! predict is identical across tools. The path is fixed: no
//! `XDG_CONFIG_HOME`, no `MDSTORE_CONFIG`, and no `$HOME` — the home
//! directory comes from the passwd database. Every environment channel
//! is repo-settable (direnv, mise, a CI wrapper), and this file names
//! where a write can land, so it is security config, held to gaff's
//! standard. Tests substitute the file through an explicit flag on the
//! consumer CLI, never through the environment.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

/// The parsed user config. An absent file is the one benign absence:
/// it means no fallback, which is exactly the behavior before this
/// file existed.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    /// The schema gate. A higher format is an error, never a guess.
    #[serde(default = "format_one")]
    pub format: u32,
    /// The user's root store: the directory of the private repo that
    /// holds the tracker, the note store, and the skill library. A
    /// read that finds no store falls back to it; a write never does.
    #[serde(default)]
    pub root_store: Option<PathBuf>,
}

fn format_one() -> u32 {
    1
}

/// The home directory, from the passwd database. Never `$HOME`.
#[must_use]
pub fn passwd_home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        nix::unistd::User::from_uid(nix::unistd::getuid())
            .ok()
            .flatten()
            .map(|u| u.dir)
    }
    #[cfg(not(unix))]
    {
        std::env::home_dir()
    }
}

/// The fixed path of the user config.
#[must_use]
pub fn config_path() -> Option<PathBuf> {
    passwd_home().map(|h| h.join(".config/mdstore/config.yml"))
}

impl UserConfig {
    /// Load the user config from its fixed path. A missing file is the
    /// default (no fallback); anything else wrong with the file is an
    /// error, never a silent downgrade to "unconfigured".
    pub fn load() -> Result<Self> {
        match config_path() {
            Some(p) => Self::load_from(&p),
            None => Ok(Self::default()),
        }
    }

    /// Load from an explicit path. The consumer CLIs expose this as a
    /// flag for tests; a flag is visible in the transcript and is not
    /// repo-settable.
    pub fn load_from(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(Error::InvalidStore(format!(
                    "cannot read {}: {e}",
                    path.display()
                )));
            }
        };
        let cfg: Self = yaml_serde::from_str(&text).map_err(|e| {
            Error::InvalidStore(format!("cannot read {}: {e}", path.display()))
        })?;
        if cfg.format > 1 {
            return Err(Error::InvalidStore(format!(
                "cannot read {}: format {} is newer than this binary understands",
                path.display(),
                cfg.format
            )));
        }
        if let Some(root) = &cfg.root_store {
            let expanded = expand_home(root).ok_or_else(|| {
                Error::InvalidStore(format!(
                    "cannot read {}: root_store starts with ~ and no home directory resolves",
                    path.display()
                ))
            })?;
            if !expanded.is_absolute() {
                return Err(Error::InvalidStore(format!(
                    "cannot read {}: root_store must be an absolute path; a relative one \
                     would mean a different store in every working directory",
                    path.display()
                )));
            }
            return Ok(Self {
                format: cfg.format,
                root_store: Some(expanded),
            });
        }
        Ok(cfg)
    }

    /// Write `root_store` to the fixed path, atomically.
    pub fn save_root(root: &Path) -> Result<PathBuf> {
        let Some(path) = config_path() else {
            return Err(Error::InvalidStore(
                "no home directory resolves from the passwd database".to_string(),
            ));
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = format!("format: 1\nroot_store: {}\n", root.display());
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }
}

/// `~` and `~/x` through the passwd home. A bare relative path passes
/// through unchanged for the caller to reject.
fn expand_home(p: &Path) -> Option<PathBuf> {
    let Ok(rest) = p.strip_prefix("~") else {
        return Some(p.to_path_buf());
    };
    passwd_home().map(|h| h.join(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mdstore-ucfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_missing_file_is_the_default_and_nothing_else_is() {
        let d = scratch("missing");
        let cfg = UserConfig::load_from(&d.join("nope.yml")).unwrap();
        assert_eq!(cfg.root_store, None);

        for (name, text, needle) in [
            ("parse", "root_store: [\n", "cannot read"),
            ("unknown", "rootstore: /x\n", "cannot read"),
            ("relative", "root_store: notes\n", "absolute"),
            ("format", "format: 2\nroot_store: /x\n", "newer"),
        ] {
            let p = d.join(format!("{name}.yml"));
            std::fs::write(&p, text).unwrap();
            let err = UserConfig::load_from(&p).unwrap_err().to_string();
            assert!(err.contains(needle), "{name}: {err}");
        }
    }

    #[test]
    fn tilde_expands_through_the_passwd_home() {
        let d = scratch("tilde");
        let p = d.join("c.yml");
        std::fs::write(&p, "root_store: ~/notes\n").unwrap();
        let cfg = UserConfig::load_from(&p).unwrap();
        let root = cfg.root_store.unwrap();
        assert!(root.is_absolute());
        assert!(root.ends_with("notes"));
        assert_eq!(root, passwd_home().unwrap().join("notes"));
    }

    #[test]
    fn the_passwd_home_ignores_the_environment() {
        // Not asserted by mutating HOME (tests share the process); the
        // contract is structural: passwd_home never reads env.
        let h = passwd_home().unwrap();
        assert!(h.is_absolute());
    }
}
