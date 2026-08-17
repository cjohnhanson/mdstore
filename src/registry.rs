//! Machine-local location overrides.
//!
//! A shared store declares its dependencies by git URL, because that is
//! what every clone can reach. On one machine the same repository is
//! often already checked out. The registry binds a declared URL to that
//! checkout, so a command reads the working copy instead of the cache.
//!
//! The registry changes *where* a dependency resolves. It never changes
//! *what* a store declares, so it cannot make one machine's links differ
//! from another's. It keys on the declared source, never on a name that
//! a dependency asserts about itself.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::store::{SourceLocator, StoreContent, StoreSource};

/// The named tool's registry file. A user overriding a URL edits the
/// directory of the tool they run, for the same reason the user config
/// does: a library's name does not belong in a config directory.
pub fn registry_path(tool: &str) -> PathBuf {
    if let Ok(p) = std::env::var("MDSTORE_REGISTRY") {
        return PathBuf::from(p);
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        });
    // `format!`, matching config_path, and not `join(tool)`. `join`
    // drops the base when the component is absolute, so the two public
    // functions would disagree on what one `tool` value can reach.
    base.join(format!("{tool}/registry.yml"))
}

/// One override: a declared source, and where it lives on this machine.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Override {
    /// The declared git URL this override answers for.
    pub git: String,
    /// The local checkout to read instead of the cache.
    pub path: PathBuf,
}

/// The parsed registry.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Registry {
    #[serde(default)]
    pub stores: Vec<Override>,
}

impl Registry {
    /// Load the named tool's registry. A missing file is an empty
    /// registry.
    pub fn load(tool: &str) -> Result<Self> {
        Self::load_from(&registry_path(tool))
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Registry::default());
        }
        let text = std::fs::read_to_string(path)?;
        Ok(yaml_serde::from_str(&text)?)
    }

    /// The local checkout bound to a declared URL.
    pub fn resolve(&self, url: &str) -> Option<&Path> {
        let wanted = crate::store::canonical_url_for_cache(url);
        self.stores
            .iter()
            .find(|o| crate::store::canonical_url_for_cache(&o.git) == wanted)
            .map(|o| o.path.as_path())
    }

    /// True when the registry binds this URL.
    pub fn overrides(&self, url: &str) -> bool {
        self.resolve(url).is_some()
    }
}

/// A locator that applies the registry, then falls back to an inner one.
///
/// It records which stores it redirected, because a check that resolves
/// through an override proves nothing about other machines: the override
/// can hold commits that were never pushed.
pub struct RegistryLocator<L: SourceLocator> {
    pub registry: Registry,
    pub inner: L,
    redirected: std::cell::RefCell<HashMap<String, PathBuf>>,
}

impl<L: SourceLocator> RegistryLocator<L> {
    pub fn new(registry: Registry, inner: L) -> Self {
        RegistryLocator {
            registry,
            inner,
            redirected: std::cell::RefCell::new(HashMap::new()),
        }
    }

    /// The URLs this locator resolved through a local checkout.
    pub fn redirected(&self) -> Vec<(String, PathBuf)> {
        let mut list: Vec<(String, PathBuf)> = self
            .redirected
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        list.sort();
        list
    }
}

impl<L: SourceLocator> SourceLocator for RegistryLocator<L> {
    fn locate(
        &self,
        source: &StoreSource,
        declaring_root: &Path,
    ) -> std::result::Result<StoreContent, String> {
        if let StoreSource::Git { url, .. } = source
            && let Some(local) = self.registry.resolve(url)
        {
            if local.is_dir() {
                self.redirected
                    .borrow_mut()
                    .insert(url.clone(), local.to_path_buf());
                return crate::confined::StoreDir::open(local)
                    .map(StoreContent::Dir)
                    .map_err(|e| e.to_string());
            }
            return Err(format!(
                "the registry binds {url} to {}, which is not a directory",
                local.display()
            ));
        }
        self.inner.locate(source, declaring_root)
    }

    fn origin_url(&self, path: &Path) -> Option<String> {
        self.inner.origin_url(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LocalPaths;

    fn tempdir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "mdstore-reg-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn each_tool_owns_its_own_registry_path() {
        // No environment is set or read here beyond what registry_path
        // already reads: two names through one call must differ, and
        // each must end in its own directory. A tool argument that the
        // body ignores collapses both, whatever the base resolves to.
        let a = registry_path("tisket");
        let b = registry_path("zettel");
        if std::env::var_os("MDSTORE_REGISTRY").is_some() {
            // The env override answers for every tool by design.
            return;
        }
        assert_ne!(a, b, "two tools must never share a registry file");
        assert!(a.ends_with("tisket/registry.yml"), "{}", a.display());
        assert!(b.ends_with("zettel/registry.yml"), "{}", b.display());
        for tool in ["tisket", "zettel", "almanac"] {
            let p = registry_path(tool);
            assert!(
                !p.ends_with("mdstore/registry.yml"),
                "{tool} reads the library's directory: {}",
                p.display()
            );
        }
    }

    #[test]
    fn a_missing_registry_is_empty() {
        let r = Registry::load_from(&PathBuf::from("/nonexistent/registry.yml")).unwrap();
        assert!(r.stores.is_empty());
    }

    #[test]
    fn an_override_matches_any_spelling_of_the_url() {
        let base = tempdir("spelling");
        let file = base.join("registry.yml");
        std::fs::write(
            &file,
            format!(
                "stores:\n  - git: https://example.com/org/kb.git\n    path: {}\n",
                base.display()
            ),
        )
        .unwrap();
        let r = Registry::load_from(&file).unwrap();
        assert!(r.overrides("https://example.com/org/kb"));
        assert!(r.overrides("git@example.com:org/kb.git"));
        assert!(!r.overrides("https://example.com/other/kb"));
    }

    #[test]
    fn the_locator_redirects_and_records_it() {
        let base = tempdir("redirect");
        let checkout = base.join("checkout");
        std::fs::create_dir_all(&checkout).unwrap();
        let registry = Registry {
            stores: vec![Override {
                git: "https://example.com/org/kb".into(),
                path: checkout.clone(),
            }],
        };
        let locator = RegistryLocator::new(registry, LocalPaths);
        let source = StoreSource::Git {
            url: "git@example.com:org/kb.git".into(),
            rev: None,
        };
        let content = locator.locate(&source, &base).unwrap();
        assert_eq!(content.dir(), Some(checkout.as_path()));
        assert_eq!(
            locator.redirected(),
            vec![("git@example.com:org/kb.git".to_string(), checkout)]
        );
    }

    #[test]
    fn an_override_that_is_not_there_is_an_error_not_a_silent_fallback() {
        let base = tempdir("missing");
        let registry = Registry {
            stores: vec![Override {
                git: "https://example.com/org/kb".into(),
                path: base.join("gone"),
            }],
        };
        let locator = RegistryLocator::new(registry, LocalPaths);
        let source = StoreSource::Git {
            url: "https://example.com/org/kb".into(),
            rev: None,
        };
        let err = locator.locate(&source, &base).unwrap_err();
        assert!(err.contains("registry binds"), "{err}");
    }

    #[test]
    fn a_path_source_is_untouched() {
        let base = tempdir("path");
        std::fs::create_dir_all(base.join("dep")).unwrap();
        let locator = RegistryLocator::new(Registry::default(), LocalPaths);
        let content = locator
            .locate(&StoreSource::Path("dep".into()), &base)
            .unwrap();
        assert_eq!(content.dir(), Some(base.join("dep").as_path()));
        assert!(locator.redirected().is_empty());
    }
}
