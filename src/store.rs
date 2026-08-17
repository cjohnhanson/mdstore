//! Store composition: named document stores that declare other stores
//! they may link into.
//!
//! A store declares its dependencies under local aliases. A reference is
//! either bare (local to the store holding the referring document) or
//! alias-qualified. Direction lives entirely in the declarations: a store
//! that never declares another can never link to it, so a shared repo
//! store cannot reference a private user store.
//!
//! This module stays vocabulary-free. It parses references, loads
//! declarations, computes the dependency closure, and guards the file
//! scan. The consumer resolves a reference to a document, because only
//! the consumer knows what an ID means.

use std::collections::HashMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The config format this build understands. A store declaring a higher
/// format is rejected rather than silently misread.
pub const FORMAT_VERSION: u32 = 2;

/// The file both tools read for store declarations. One map per repo, so
/// two tools in one repo cannot drift into separate alias graphs.
pub const STORES_FILE: &str = "stores.yml";

// -- References --

/// A reference to a document, possibly in another store.
///
/// `alias` is empty for a local reference. More than one element is the
/// read-only path form (`project/shared-kb:b7c1`) that commands print for
/// a document reached through a dependency's own dependency.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoreRef {
    pub alias: Vec<String>,
    pub id: String,
}

impl StoreRef {
    /// A reference local to the store holding the referring document.
    pub fn local(id: impl Into<String>) -> Self {
        StoreRef {
            alias: Vec::new(),
            id: id.into(),
        }
    }

    /// Parse a reference against the set of aliases the referring store
    /// declares.
    ///
    /// The discrimination rule: text before the first colon is an alias
    /// only when it is declared. Everything else is an opaque ID, so a
    /// DOI (`10.1145/12345`), a legacy `depends_on` entry (`x: y`), and
    /// any other colon-bearing string keep their current meaning.
    pub fn parse(s: &str, declared: &dyn AliasSet) -> Self {
        let Some((head, rest)) = s.split_once(':') else {
            return StoreRef::local(s);
        };
        // Path form: every segment must be a plausible alias, and only
        // the first is checked against the declaring store. The
        // traversal resolves the other segments store by store.
        let segments: Vec<&str> = head.split('/').collect();
        if segments.iter().all(|seg| is_alias_shaped(seg))
            && !rest.is_empty()
            && declared.declares(segments[0])
        {
            return StoreRef {
                alias: segments.iter().map(|s| s.to_string()).collect(),
                id: rest.to_string(),
            };
        }
        StoreRef::local(s)
    }

    /// True when the reference points outside the referring store.
    pub fn is_foreign(&self) -> bool {
        !self.alias.is_empty()
    }
}

impl fmt::Display for StoreRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.alias.is_empty() {
            write!(f, "{}:", self.alias.join("/"))?;
        }
        f.write_str(&self.id)
    }
}

/// An alias must look like an alias for the discrimination rule to fire.
/// This keeps `10.1145/12345` and `x: y` opaque even if someone declares
/// an alias with an unusual name.
fn is_alias_shaped(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The aliases a store declares. Implemented by [`StoresConfig`], and by
/// consumers that keep their own table.
pub trait AliasSet {
    fn declares(&self, alias: &str) -> bool;
}

/// No aliases: every reference parses as local.
pub struct NoAliases;

impl AliasSet for NoAliases {
    fn declares(&self, _alias: &str) -> bool {
        false
    }
}

impl AliasSet for Vec<String> {
    fn declares(&self, alias: &str) -> bool {
        self.iter().any(|a| a == alias)
    }
}

// -- Declarations --

/// Where a store's documents come from.
///
/// Written flat in `stores.yml`, so a declaration reads as one line plus
/// optional keys: `path: ../project`, or `git: <url>` with `rev: <rev>`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "SourceFields", into = "SourceFields")]
pub enum StoreSource {
    /// A directory on this machine.
    Path(PathBuf),
    /// A git repository. Read-only, fetched into a bare cache.
    Git { url: String, rev: Option<String> },
    /// Objects under one prefix in object storage. Read-only, copied
    /// into the cache by an explicit sync.
    Blob { url: String },
}

/// The on-disk shape of a source declaration.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct SourceFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    git: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rev: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blob: Option<String>,
}

impl TryFrom<SourceFields> for StoreSource {
    type Error = String;

    fn try_from(f: SourceFields) -> std::result::Result<Self, String> {
        let kinds = [f.path.is_some(), f.git.is_some(), f.blob.is_some()]
            .iter()
            .filter(|present| **present)
            .count();
        if kinds > 1 {
            return Err("a store declares one of 'path', 'git', or 'blob'".into());
        }
        if let Some(p) = f.path {
            if f.rev.is_some() {
                return Err("'rev' applies to a git source, not a path".into());
            }
            return Ok(StoreSource::Path(p));
        }
        if let Some(url) = f.git {
            return Ok(StoreSource::Git { url, rev: f.rev });
        }
        if let Some(url) = f.blob {
            if f.rev.is_some() {
                return Err("'rev' applies to a git source, not a blob".into());
            }
            return Ok(StoreSource::Blob { url });
        }
        Err("a store needs a 'path', 'git', or 'blob' source".into())
    }
}

impl From<StoreSource> for SourceFields {
    fn from(s: StoreSource) -> Self {
        match s {
            StoreSource::Path(p) => SourceFields {
                path: Some(p),
                git: None,
                rev: None,
                blob: None,
            },
            StoreSource::Git { url, rev } => SourceFields {
                path: None,
                git: Some(url),
                rev,
                blob: None,
            },
            StoreSource::Blob { url } => SourceFields {
                path: None,
                git: None,
                rev: None,
                blob: Some(url),
            },
        }
    }
}

/// True when a declared path resolves outside the repository.
///
/// The text of the path is checked separately. This asks the
/// filesystem, so a link inside the repository that points out of it
/// is caught as well.
fn resolves_outside(repo_root: &Path, declared: &Path) -> bool {
    let (Ok(root), Ok(target)) = (
        repo_root.canonicalize(),
        repo_root.join(declared).canonicalize(),
    ) else {
        // A path that does not resolve is reported by the reachability
        // check instead, and reporting it twice helps nobody.
        return false;
    };
    !target.starts_with(&root)
}

/// The location on this machine that a source names, if it names one.
///
/// A key proves nothing about a value. Git clones a local directory as
/// readily as a URL, so `git: /home/you/private` names this machine
/// exactly as `path: /home/you/private` does. Every guard about
/// locations must therefore ask this, not `matches!(source, Path(_))`.
pub(crate) fn on_machine_location(source: &StoreSource) -> Option<PathBuf> {
    match source {
        StoreSource::Path(p) => Some(p.clone()),
        // Ask the parser that performs the fetch. Stripping a literal
        // "file://" prefix answered a different question: it missed
        // "FILE://", and it read "file://localhost/abs/private" as the
        // relative path "localhost/abs/private" while gix resolves it
        // to "/abs/private" and clones the private repository.
        StoreSource::Git { url, .. } => crate::git::local_path(url),
        // Blob has its own scheme set, so a value that is not one of
        // those names a location here.
        StoreSource::Blob { url } => {
            if crate::blob::scheme_of(url).is_some() {
                return None;
            }
            crate::git::local_path(url).or_else(|| Some(PathBuf::from(url)))
        }
    }
}

/// Why a declared path names one machine, if it does.
///
/// A relative path is read against the store that declared it, so it
/// keeps its meaning wherever that store is copied. These two do not.
fn anchored_to_one_machine(declared: &Path) -> Option<&'static str> {
    if declared.is_absolute() {
        return Some("an absolute path");
    }
    if declared.starts_with("~") {
        return Some("a home-anchored path");
    }
    None
}

/// True when the authority of a URL names this machine.
///
/// The host is whatever precedes the first `/`, `?` or `#`, minus any
/// userinfo and port.
fn host_is_local(authority: &str) -> bool {
    // Parse the authority with the parser that performs the request,
    // never by hand. std's IpAddr accepts a dotted quad and nothing
    // else, so the hand-written test called `127.1`, `2130706433`,
    // `0x7f000001` and `0177.0.0.1` remote. The URL parser applies the
    // WHATWG rules and resolves all four to 127.0.0.1, so the fetch
    // reached the reader's own machine through a guard written to
    // refuse exactly that.
    let Ok(parsed) = url::Url::parse(&format!("http://{authority}")) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Domain(name)) => {
            name.trim_end_matches('.').eq_ignore_ascii_case("localhost")
        }
        Some(url::Host::Ipv4(v4)) => address_is_local(std::net::IpAddr::V4(v4)),
        Some(url::Host::Ipv6(v6)) => match v6.to_ipv4_mapped() {
            // `[::ffff:127.0.0.1]` reaches the same service as
            // `127.0.0.1`, and is_loopback answers false for it.
            Some(v4) => address_is_local(std::net::IpAddr::V4(v4)),
            None => address_is_local(std::net::IpAddr::V6(v6)),
        },
        None => false,
    }
}

/// True when an address reaches a service on this machine.
///
/// Loopback is the obvious half. The unspecified address is the other:
/// a connection to `0.0.0.0` or `[::]` reaches a local listener, so a
/// declaration naming it is a declaration naming this machine.
fn address_is_local(ip: std::net::IpAddr) -> bool {
    ip.is_loopback() || ip.is_unspecified()
}

/// True when a source names a location this machine reaches over a
/// network, and false for anything that resolves on this machine.
///
/// A store that a stranger publishes may declare only the first kind.
/// `git:` and `blob:` accept a local directory, `file://`, and a
/// relative path, so the variant a declaration uses proves nothing.
pub fn declares_a_remote_location(source: &StoreSource) -> bool {
    match source {
        StoreSource::Path(_) => false,
        StoreSource::Git { url, .. } | StoreSource::Blob { url } => is_remote_url(url),
    }
}

/// True for `scheme://host/...` with a scheme that is not `file`, and
/// for the scp form `user@host:path` that git accepts.
///
/// Everything else is a location on this machine: an absolute path, a
/// relative path, a bare directory name, and `file:///...`.
pub fn is_remote_url(value: &str) -> bool {
    if let Some((scheme, rest)) = value.split_once("://") {
        if scheme.eq_ignore_ascii_case("file")
            || scheme.is_empty()
            || !scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
            || rest.is_empty()
        {
            return false;
        }
        // A loopback host is this machine wearing a URL. The scheme
        // alone said "remote", so http://127.0.0.1/... named a service
        // on the reader's own machine and passed the guard.
        return !host_is_local(rest);
    }
    // The scp form: user@host:path, with a host that is not a Windows
    // drive letter and a path that does not start the string.
    if let Some((before, after)) = value.split_once(':') {
        if after.starts_with('/') && before.len() == 1 {
            return false; // C:/... is a path
        }
        if let Some((_user, host)) = before.split_once('@') {
            return !host.is_empty() && !host.contains('/') && !host_is_local(host);
        }
    }
    false
}

impl StoreSource {
    /// The identity of the store this source points at.
    ///
    /// Identity is the source itself, never a self-declared name: a name
    /// in a dependency's own config is unverifiable, and every store
    /// written before this feature has none. A path whose repository has
    /// an origin remote takes that remote's identity, so the same store
    /// reached by path here and by URL there is one closure member, not
    /// two.
    ///
    /// `located` is where the source resolved on this machine. A path
    /// identity must come from the resolved location, never the declared
    /// text. `../b` in two different stores names two different
    /// directories. `../a` in a dependency names the root store itself,
    /// which is how a mutual cycle closes.
    pub fn identity(&self, located: Option<&Path>, origin_of: &dyn OriginLookup) -> StoreId {
        // A URL identifies a store wherever it is read from. A local
        // location does not: `git: ../shared` in two different stores
        // names two different directories, and keying both on the
        // declared text collapsed them into one member, so a reference
        // in the second store resolved into the first store's
        // documents. Only a real URL takes the URL key; every local
        // spelling falls through to the resolved-location key below.
        match self {
            StoreSource::Git { url, .. } if is_remote_url(url) => {
                return StoreId(canonical_url(url));
            }
            StoreSource::Blob { url } if is_remote_url(url) => {
                return StoreId(format!("blob:{}", canonical_url(url)));
            }
            _ => {}
        }
        let Some(dir) = located else {
            // Unresolvable: fall back to the declared text so repeated
            // declarations of the same missing path stay one member.
            let declared = match self {
                StoreSource::Path(p) => p.display().to_string(),
                StoreSource::Git { url, .. } | StoreSource::Blob { url } => url.clone(),
            };
            return StoreId(format!("unresolved:{declared}"));
        };
        if let Some(url) = origin_of.origin_url(dir) {
            return StoreId(canonical_url(&url));
        }
        let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        StoreId(format!("path:{}", canonical.display()))
    }
}

/// The identity of one closure member, with the revision it reads.
///
/// Two checkouts of one repository share an origin remote, so they
/// share the identity above and the dedup collapses them into one
/// member. That is right when they read the same content and wrong
/// when they do not: a stable checkout and a feature checkout are two
/// sets of documents, and the second alias silently read the first
/// one's notes.
///
/// The revision separates them. A git tree carries the revision it was
/// resolved at; a checkout carries its HEAD. A member with no
/// revision, such as a plain directory that is not a repository, keeps
/// the bare identity.
///
/// One function answers for every source kind, so a new call site
/// cannot reintroduce the URL-only key for one of them.
pub fn member_identity(
    source: &StoreSource,
    content: Option<&StoreContent>,
    located: Option<&Path>,
    origin_of: &dyn OriginLookup,
) -> StoreId {
    let base = source.identity(located, origin_of);
    let rev = match content {
        Some(StoreContent::GitTree { rev, .. }) => Some(rev.clone()),
        // A checkout that is a repository reads whatever its working
        // tree holds, and HEAD is the closest stable name for that.
        Some(StoreContent::Dir(dir)) => origin_of
            .origin_url(dir.root())
            .and_then(|_| crate::git::resolve_rev(dir.root(), None).ok()),
        None => None,
    };
    match rev {
        Some(rev) => StoreId(format!("{}@{rev}", base.0)),
        None => base,
    }
}

/// Looks up the origin remote of a checkout, so a path source and a git
/// source for one repository share an identity. The consumer implements
/// it. [`NoOrigins`] disables the lookup.
pub trait OriginLookup {
    fn origin_url(&self, path: &Path) -> Option<String>;
}

/// Every path keeps a path identity.
pub struct NoOrigins;

impl OriginLookup for NoOrigins {
    fn origin_url(&self, _path: &Path) -> Option<String> {
        None
    }
}

/// The canonical URL, for the git cache to key its clones by.
pub(crate) fn canonical_url_for_cache(url: &str) -> String {
    canonical_url(url)
}

/// Normalize a remote URL so the same repository written different ways
/// compares equal.
fn canonical_url(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    // git@host:path and https://host/path name the same repository. A
    // remote spelling folds and lowercases; a local path is neither.
    if let Some((_, rest)) = u.split_once("://") {
        return rest.to_ascii_lowercase();
    }
    // scp form is user@host:path: a user name with no slash before
    // the @, and a colon after it. An @ after a slash is a character
    // in a path, and an @ with no colon is too: up@2 read as scp kept
    // only "2", so two such names shared a slot, and the lowercasing
    // merged local repositories that differ by case. git's own rule
    // is the colon.
    if let Some((user, rest)) = u.split_once('@')
        && !user.contains('/')
        && rest.contains(':')
    {
        return rest.replacen(':', "/", 1).to_ascii_lowercase();
    }
    u.to_string()
}

/// The canonical identity of a store.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StoreId(pub String);

impl fmt::Display for StoreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One store's declarations, read from `stores.yml`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StoresConfig {
    /// The config format. Absent means 1 (pre-composition).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<u32>,
    /// Optional display name. Never an identity: see [`StoreSource::identity`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// True when other people clone this store.
    ///
    /// A shared store's dependencies must be reachable for everyone who
    /// clones it, so [`StoresConfig::unshareable`] applies. A private
    /// store can declare local paths freely. A personal knowledge base
    /// stays on one machine, and local paths are the reason to have one.
    /// The config declares this state. Zettel does not calculate it,
    /// because a private store and a shared store are both git
    /// repositories.
    #[serde(default)]
    pub shared: bool,
    /// Dependencies by local alias, in declaration order.
    #[serde(default)]
    pub stores: Vec<StoreDecl>,
}

/// One dependency declaration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StoreDecl {
    pub alias: String,
    #[serde(flatten)]
    pub source: StoreSource,
}

impl AliasSet for StoresConfig {
    fn declares(&self, alias: &str) -> bool {
        self.stores.iter().any(|d| d.alias == alias)
    }
}

impl StoresConfig {
    /// Load `stores.yml` from a store root. A missing file is an empty
    /// config, so every repo written before this feature keeps working.
    pub fn load(root: &Path) -> Result<Self> {
        Self::load_from(&StoreContent::Dir(crate::confined::StoreDir::open(root)?))
    }

    /// Load `stores.yml` from a store, local or remote.
    pub fn load_from(content: &StoreContent) -> Result<Self> {
        // Absent and refused are different answers. A store with no
        // declarations is ordinary; a stores.yml that is a symlink is
        // a store whose declarations were dropped, and reading that as
        // "declares nothing" hid every dependency it had.
        if !content.exists(STORES_FILE) {
            if content.present_but_irregular(STORES_FILE) {
                return Err(Error::InvalidStore(format!(
                    "{STORES_FILE} is not a regular file"
                )));
            }
            return Ok(StoresConfig::default());
        }
        let text = content.read(STORES_FILE)?;
        let config: StoresConfig = yaml_serde::from_str(&text)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if let Some(format) = self.format
            && format > FORMAT_VERSION
        {
            return Err(Error::UnsupportedFormat {
                found: format,
                supported: FORMAT_VERSION,
            });
        }
        let mut seen: Vec<&str> = Vec::new();
        for decl in &self.stores {
            if !is_alias_shaped(&decl.alias) {
                return Err(Error::InvalidStore(format!(
                    "alias '{}' must be alphanumeric with - or _",
                    decl.alias
                )));
            }
            if seen.contains(&decl.alias.as_str()) {
                return Err(Error::InvalidStore(format!(
                    "alias '{}' is declared twice",
                    decl.alias
                )));
            }
            seen.push(&decl.alias);
        }
        Ok(())
    }

    /// The source declared under an alias.
    pub fn source(&self, alias: &str) -> Option<&StoreSource> {
        self.stores
            .iter()
            .find(|d| d.alias == alias)
            .map(|d| &d.source)
    }

    /// The declared aliases, in order.
    pub fn aliases(&self) -> Vec<String> {
        self.stores.iter().map(|d| d.alias.clone()).collect()
    }

    /// Dependencies a shared store declares that other clones cannot
    /// follow.
    ///
    /// A shared store's link targets must be reachable for everyone who
    /// clones it. A path leaving the repository is reachable only on
    /// the declaring machine: an absolute or home-anchored path names a
    /// different directory for every other user.
    ///
    /// This reports; it does not refuse. The declaring machine can
    /// still resolve such a path, and refusing it there would break the
    /// author's own store to protect a reader who has not cloned it
    /// yet. `check` names each one.
    pub fn unshareable(&self, repo_root: &Path) -> Vec<(String, String)> {
        let mut bad = Vec::new();
        if !self.shared {
            return bad;
        }
        for decl in &self.stores {
            // A git or blob declaration whose value names this machine
            // is a local path wearing another key, and every clone of
            // this store would follow it somewhere else or nowhere.
            //
            // One resolver answers here and at the walk's own guard. A
            // second copy of this reasoning is how the two came to
            // disagree about `FILE://` and about a file URL with a
            // host component.
            let Some(p) = on_machine_location(&decl.source) else {
                continue;
            };
            let p = &p;
            let reason = if p.is_absolute() {
                Some("absolute path".to_string())
            } else if p.starts_with("~") {
                Some("home-anchored path".to_string())
            } else if escapes_root(p) {
                Some("path leaves the repository".to_string())
            } else if resolves_outside(repo_root, p) {
                // The text stays inside the repository and the
                // filesystem does not: a link committed to the repo
                // points wherever the declaring machine put it, and
                // every other clone follows it somewhere else or
                // nowhere.
                Some("path resolves outside the repository through a link".to_string())
            } else {
                None
            };
            if let Some(reason) = reason {
                bad.push((
                    decl.alias.clone(),
                    format!(
                        "{reason} — a shared store must declare an outside \
                         dependency by git URL so every clone can reach it \
                         (repo root: {})",
                        repo_root.display()
                    ),
                ));
            }
        }
        bad
    }
}

/// True when a relative path climbs out of its base.
fn escapes_root(p: &Path) -> bool {
    let mut depth: i32 = 0;
    for c in p.components() {
        match c {
            Component::ParentDir => depth -= 1,
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return true,
        }
        if depth < 0 {
            return true;
        }
    }
    false
}

// -- The dependency graph --

/// One member of a dependency closure.
#[derive(Debug, Clone)]
pub struct Member {
    pub id: StoreId,
    /// How this member is named from the root: empty for the root store,
    /// `["project"]` for a direct dependency, `["project", "shared-kb"]`
    /// for one reached through it.
    pub alias_path: Vec<String>,
    pub source: StoreSource,
    /// Where the documents are, when the source could be located.
    pub content: Option<StoreContent>,
    /// Why the member could not be loaded, if it could not.
    pub unavailable: Option<String>,
    /// True when this member came from a remote source, or from a
    /// member that did. A remote store is content that somebody else
    /// controls, so it may not name a directory on this machine.
    pub remote: bool,
}

impl Member {
    /// How a document in this member prints from the root's vantage.
    pub fn qualify(&self, id: &str) -> String {
        if self.alias_path.is_empty() {
            id.to_string()
        } else {
            format!("{}:{}", self.alias_path.join("/"), id)
        }
    }
}

/// Locates a declared source on this machine. Consumers implement it so
/// that git fetching and machine-local overrides stay out of this module.
pub trait SourceLocator {
    /// Where a source's documents can be read, or why they cannot.
    fn locate(
        &self,
        source: &StoreSource,
        declaring_root: &Path,
    ) -> std::result::Result<StoreContent, String>;
    /// The origin remote of a checkout, for identity.
    fn origin_url(&self, _path: &Path) -> Option<String> {
        None
    }
}

impl<T: SourceLocator> OriginLookup for T {
    fn origin_url(&self, path: &Path) -> Option<String> {
        SourceLocator::origin_url(self, path)
    }
}

/// Where one store's documents are.
///
/// A local store reads from a directory. A remote store reads from git
/// objects at one revision, because a bare clone has no working tree.
/// That is what lets two consumers pin different revisions of one URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreContent {
    /// A directory on this machine, held open.
    ///
    /// The handle, not the path, is what the store keeps. A path would
    /// have to be re-opened for each operation, and each re-open
    /// resolves through ambient authority, so a root swapped between
    /// two reads is followed.
    Dir(crate::confined::StoreDir),
    GitTree {
        /// The bare clone.
        cache: PathBuf,
        /// The resolved commit. Fixed when the graph opens, so one
        /// command sees one state of the store.
        rev: String,
        /// The URL, for messages.
        url: String,
    },
}

impl StoreContent {
    /// Read a file, by a path relative to the store root.
    ///
    /// A store holds content that somebody else may control, so a file
    /// that is not a regular file is refused rather than followed. A
    /// git tree needs no such check: it holds objects, not links into
    /// this machine.
    pub fn read(&self, rel: &str) -> Result<String> {
        match self {
            StoreContent::Dir(dir) => dir.read(rel),
            StoreContent::GitTree { cache, rev, .. } => crate::git::show(cache, rev, rel),
        }
    }

    /// True when the store holds this file as a regular file.
    pub fn exists(&self, rel: &str) -> bool {
        match self {
            StoreContent::Dir(dir) => dir.is_document(rel),
            StoreContent::GitTree { cache, rev, .. } => crate::git::show(cache, rev, rel).is_ok(),
        }
    }

    /// True when the path exists but is not a regular file.
    ///
    /// A directory read skips a link by type, so this is what tells a
    /// caller that something was there and was refused.
    #[must_use]
    pub fn present_but_irregular(&self, rel: &str) -> bool {
        match self {
            StoreContent::Dir(dir) => dir.present_but_irregular(rel),
            // A git tree holds objects, not links into a filesystem.
            StoreContent::GitTree { .. } => false,
        }
    }

    /// The subdirectory names under one directory of this store.
    ///
    /// A store may be content somebody else controls, so a link is
    /// skipped by dirent type rather than followed. One implementation
    /// answers for a local directory and for a git tree: a copy per
    /// consumer is a copy that can miss the link test, which is how a
    /// symlinked project directory let a tracker read and write
    /// outside its own root.
    ///
    /// Names starting with a dot are omitted. The result is sorted.
    #[must_use]
    pub fn subdirectories(&self, subdir: &str) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        match self {
            StoreContent::Dir(root) => names.extend(root.subdirectories(subdir)),
            StoreContent::GitTree { cache, rev, .. } => {
                // A git tree holds objects, so no link can leave it.
                if let Ok(paths) = crate::git::list_tree(cache, rev, subdir) {
                    for path in paths {
                        if let Some((name, _)) = path.split_once('/')
                            && !name.starts_with('.')
                            && !names.iter().any(|n| n == name)
                        {
                            names.push(name.to_string());
                        }
                    }
                }
            }
        }
        names.sort();
        names
    }

    /// The `.md` files of a subdirectory, with their stems.
    ///
    /// A directory scan skips every entry that is not a regular file. A
    /// git tree gives its entries from objects, so a symlink in a remote
    /// store cannot reach a file outside the repository.
    pub fn scan(&self, subdir: &str) -> Result<Scan> {
        match self {
            StoreContent::Dir(root) => root.scan(subdir),
            StoreContent::GitTree { cache, rev, .. } => {
                let mut scan = Scan::default();
                let mut names = crate::git::list_tree(cache, rev, subdir)?;
                names.sort();
                for name in names {
                    if !name.ends_with(".md") || name.contains('/') {
                        continue;
                    }
                    scan.entries.push(ScanEntry {
                        path: PathBuf::from(subdir).join(&name),
                        stem: name.trim_end_matches(".md").to_string(),
                    });
                }
                Ok(scan)
            }
        }
    }

    /// A directory on this machine, if the store has one.
    pub fn dir(&self) -> Option<&Path> {
        match self {
            StoreContent::Dir(d) => Some(d.root()),
            StoreContent::GitTree { .. } => None,
        }
    }

    /// True when a remote cache holds the store.
    pub fn is_remote(&self) -> bool {
        matches!(self, StoreContent::GitTree { .. })
    }

    /// The time since the last fetch, for a remote store. A stale answer
    /// must look stale.
    pub fn fetch_age(&self) -> Option<String> {
        match self {
            StoreContent::Dir(_) => None,
            StoreContent::GitTree { cache, .. } => {
                crate::git::seconds_since_fetch(cache).map(crate::git::humanize_age)
            }
        }
    }
}

/// Resolves declared sources with no network access: a path resolves
/// relative to the declaring store, and a git source resolves only if
/// its clone is already in the cache.
pub struct LocalPaths;

impl SourceLocator for LocalPaths {
    fn locate(
        &self,
        source: &StoreSource,
        declaring_root: &Path,
    ) -> std::result::Result<StoreContent, String> {
        match source {
            StoreSource::Path(p) => {
                if !p.is_absolute() && declaring_root.as_os_str().is_empty() {
                    return Err(format!(
                        "'{}' is relative and the declaring store has no directory",
                        p.display()
                    ));
                }
                let joined = if p.is_absolute() {
                    p.clone()
                } else {
                    declaring_root.join(p)
                };
                if joined.is_dir() {
                    crate::confined::StoreDir::open(&joined)
                        .map(StoreContent::Dir)
                        .map_err(|e| e.to_string())
                } else {
                    Err(format!("no directory at {}", joined.display()))
                }
            }
            StoreSource::Blob { url } => {
                let dir = crate::blob::locate(url)?;
                crate::confined::StoreDir::open(&dir)
                    .map(StoreContent::Dir)
                    .map_err(|e| e.to_string())
            }
            StoreSource::Git { url, rev } => {
                if !crate::git::is_cached(url) {
                    return Err(format!("{url} is not in the cache; run store sync"));
                }
                let cache = crate::git::cache_dir(url);
                let resolved = crate::git::resolve_rev(&cache, rev.as_deref())
                    .map_err(|e| format!("{url}: {e}"))?;
                Ok(StoreContent::GitTree {
                    cache,
                    rev: resolved,
                    url: url.clone(),
                })
            }
        }
    }

    fn origin_url(&self, path: &Path) -> Option<String> {
        crate::git::origin_url(path)
    }
}

/// Resolves declared sources and clones a git source that is absent.
/// Only a command that the user expects to reach the network uses this.
pub struct FetchingLocator;

impl SourceLocator for FetchingLocator {
    fn locate(
        &self,
        source: &StoreSource,
        declaring_root: &Path,
    ) -> std::result::Result<StoreContent, String> {
        if let StoreSource::Git { url, .. } = source {
            crate::git::ensure_clone(url).map_err(|e| e.to_string())?;
        }
        LocalPaths.locate(source, declaring_root)
    }

    fn origin_url(&self, path: &Path) -> Option<String> {
        crate::git::origin_url(path)
    }
}

/// The dependency closure of a root store.
///
/// Members are deduplicated by identity and ordered breadth-first in
/// declaration order, so output is stable. Cycles terminate: a store
/// already in the closure is not walked twice, which makes mutual
/// declarations between two shareable stores legal.
#[derive(Debug)]
pub struct StoreGraph {
    pub members: Vec<Member>,
    /// Alias tables by member index, for per-store reference resolution.
    configs: Vec<StoresConfig>,
    /// (declaring member, alias) -> target member. Built during the walk,
    /// so every alias resolves through the config that declared it.
    targets: HashMap<(usize, String), usize>,
    /// Conflicts found while walking: two members declaring the same
    /// name, or a declaration that could not be located.
    pub findings: Vec<String>,
}

impl StoreGraph {
    /// Walk the declarations from a root store.
    pub fn open(root: &Path, locator: &dyn SourceLocator) -> Result<Self> {
        let root_config = StoresConfig::load(root)?;
        // The root takes its identity the same way every other member
        // does, revision included. A root keyed without the revision
        // matched a dependency that carried one, or failed to match a
        // cycle that closed back onto it.
        let root_content = StoreContent::Dir(crate::confined::StoreDir::open(root)?);
        let root_id = member_identity(
            &StoreSource::Path(root.to_path_buf()),
            Some(&root_content),
            Some(root),
            &LookupAdapter(locator),
        );

        let mut members = vec![Member {
            id: root_id,
            alias_path: Vec::new(),
            source: StoreSource::Path(root.to_path_buf()),
            content: Some(root_content.clone()),
            unavailable: None,
            remote: false,
        }];
        let mut configs = vec![root_config];
        let mut findings = Vec::new();
        let mut names: HashMap<String, StoreId> = HashMap::new();
        let mut targets: HashMap<(usize, String), usize> = HashMap::new();

        // Breadth-first so a nearer declaration wins the alias path.
        let mut cursor = 0;
        while cursor < members.len() {
            let (decls, declaring_root) = {
                let member = &members[cursor];
                // A remote store declares its own dependencies by URL,
                // never by a path, so a store with no local directory
                // resolves its declarations against its cache root.
                let member_root = match &member.content {
                    Some(c) => c.dir().map(|d| d.to_path_buf()).unwrap_or_default(),
                    None => {
                        cursor += 1;
                        continue;
                    }
                };
                (configs[cursor].stores.clone(), member_root)
            };
            let parent_path = members[cursor].alias_path.clone();

            let parent_remote = members[cursor].remote;

            for decl in decls {
                let mut alias_path = parent_path.clone();
                alias_path.push(decl.alias.clone());

                // A remote store is content somebody else controls. If
                // it could name a location on this machine, publishing
                // a store would be enough to pull a reader's private
                // files into their own closure.
                //
                // The test is the value, never the key it was written
                // under. Git clones a local directory as readily as a
                // URL, so `git: /home/you/private` is the same attack
                // as `path: /home/you/private` with a different
                // spelling.
                if parent_remote && !declares_a_remote_location(&decl.source) {
                    findings.push(format!(
                        "store '{}' is remote and may not declare a location on this machine; \
                         declare an outside dependency by URL",
                        alias_path.join("/")
                    ));
                    continue;
                }

                // The same published content, checked out locally and
                // declared with `path:`, is the same third-party
                // content. Its declarations reach whatever they name.
                //
                // A relative path stays in the working area the reader
                // laid out, and travels with a copy of it. An absolute
                // or home-anchored path names one machine, so it is
                // never portable and it is exactly how a vendored store
                // would name a reader's private one. The root is
                // exempt: a person declaring their own dependencies may
                // name anything on their own machine.
                if cursor != 0
                    && let Some(p) = on_machine_location(&decl.source)
                    && let Some(reason) = anchored_to_one_machine(&p)
                {
                    findings.push(format!(
                        "store '{}' declares {reason} ('{}'); a dependency must declare a \
                         relative path or a URL, because it cannot know this machine",
                        alias_path.join("/"),
                        p.display()
                    ));
                    continue;
                }
                let child_remote = parent_remote
                    || matches!(
                        decl.source,
                        StoreSource::Git { .. } | StoreSource::Blob { .. }
                    );

                // A relative local git declaration names a repository
                // next to the store that declared it. The declared
                // text reached the cache untouched, so `git: ../up` in
                // two roots keyed one slot and the second root read
                // the first root's mirror, and the fetch resolved the
                // path against the process cwd. Resolved here, after
                // the guards, so a dependency's declaration is still
                // judged on what it wrote, and everything downstream —
                // locate, identity, the consumer's sync — sees one
                // absolute path.
                let decl_source = match &decl.source {
                    // A scheme is not a relative path. is_remote_url
                    // is deliberately false for file://, and joining
                    // that text onto the root mangled a legitimate
                    // root-level declaration into a member no sync
                    // could satisfy.
                    StoreSource::Git { url, rev }
                        if !is_remote_url(url) && !url.contains("://") =>
                    {
                        let joined = declaring_root.join(url);
                        let resolved = joined.canonicalize().unwrap_or(joined);
                        StoreSource::Git {
                            url: resolved.display().to_string(),
                            rev: rev.clone(),
                        }
                    }
                    other => other.clone(),
                };

                let located = locator.locate(&decl_source, &declaring_root);
                let (content, unavailable) = match located {
                    Ok(c) => (Some(c), None),
                    Err(why) => {
                        findings.push(format!(
                            "store '{}' unavailable: {why}",
                            alias_path.join("/")
                        ));
                        (None, Some(why))
                    }
                };
                // Two consumers may read one repository at different
                // revisions, whether they pin it or check it out
                // twice. They are one fetch and two sets of documents,
                // so the closure holds two members.
                //
                // A git source that names this machine has no store
                // directory, because its content is read from the
                // clone's objects. Its resolved location is still the
                // thing that identifies it, so pass that: without it
                // the identity fell back to the declared text, and two
                // stores each declaring the same relative path became
                // one member.
                let located_at = content
                    .as_ref()
                    .and_then(|c| c.dir().map(Path::to_path_buf))
                    .or_else(|| on_machine_location(&decl_source));
                let id = member_identity(
                    &decl_source,
                    content.as_ref(),
                    located_at.as_deref(),
                    &LookupAdapter(locator),
                );

                if let Some(existing) = members.iter().position(|m| m.id == id) {
                    // Already in the closure under a nearer alias path,
                    // or it is the root itself: one store, one member.
                    // This is what makes a mutual cycle terminate. The
                    // alias still resolves to that existing member.
                    targets.insert((cursor, decl.alias.clone()), existing);
                    continue;
                }
                targets.insert((cursor, decl.alias.clone()), members.len());

                let config = match &content {
                    Some(c) => match StoresConfig::load_from(c) {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            findings.push(format!(
                                "store '{}' has an unreadable {STORES_FILE}: {e}",
                                alias_path.join("/")
                            ));
                            StoresConfig::default()
                        }
                    },
                    None => StoresConfig::default(),
                };

                if let Some(name) = &config.name {
                    match names.get(name) {
                        Some(other) if other != &id => findings.push(format!(
                            "two stores declare the name '{name}' ({other} and {id})"
                        )),
                        _ => {
                            names.insert(name.clone(), id.clone());
                        }
                    }
                }

                members.push(Member {
                    id,
                    alias_path,
                    source: decl_source.clone(),
                    content,
                    unavailable,
                    remote: child_remote,
                });
                configs.push(config);
            }
            cursor += 1;
        }

        Ok(StoreGraph {
            members,
            configs,
            targets,
            findings,
        })
    }

    /// The root store: the vantage every command runs from.
    pub fn root(&self) -> &Member {
        &self.members[0]
    }

    /// The alias table of a member, for resolving references written in
    /// that member's documents. A reference is always resolved through
    /// the config of the store that contains it, never the vantage's.
    pub fn config(&self, index: usize) -> &StoresConfig {
        &self.configs[index]
    }

    /// The member an alias names, as written in `from`'s documents.
    ///
    /// The alias table is always the declaring store's own. Two stores
    /// can declare the same alias for different targets. A reference in
    /// one store never resolves through the table of the other store.
    pub fn target_of(&self, from: usize, alias: &str) -> Option<usize> {
        self.targets.get(&(from, alias.to_string())).copied()
    }

    /// Members that could not be loaded, by alias path.
    pub fn unavailable(&self) -> Vec<&Member> {
        self.members
            .iter()
            .filter(|m| m.unavailable.is_some())
            .collect()
    }
}

/// Bridges a `SourceLocator` into the `OriginLookup` identity uses.
struct LookupAdapter<'a>(&'a dyn SourceLocator);

impl OriginLookup for LookupAdapter<'_> {
    fn origin_url(&self, path: &Path) -> Option<String> {
        self.0.origin_url(path)
    }
}

// -- The file scan guard --

/// One entry of a guarded scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanEntry {
    /// Where the document is, relative to the store root.
    ///
    /// Relative since 0.3.0. It was absolute, and a consumer that
    /// passed it to std::fs kept working by accident while the store
    /// it belonged to was no longer the one that answered.
    pub path: PathBuf,
    pub stem: String,
}

/// The result of scanning a store directory.
#[derive(Debug, Default)]
pub struct Scan {
    pub entries: Vec<ScanEntry>,
    /// Paths skipped by a guard, with the reason. Never silent: a
    /// consumer's `check` command reports these.
    ///
    /// Relative to the scanned store since 0.3.0, like
    /// [`ScanEntry::path`]. The old scan pushed the absolute entry
    /// path, and a consumer printing these saw its output change
    /// meaning on that release without a changelog line.
    pub skipped: Vec<(PathBuf, String)>,
}

/// Resolve a store's document directory, rejecting a configured value
/// that is absolute or climbs out of the store root.
///
/// A dependency store holds third-party content. Its config must not
/// point the reader at a directory outside the store.
pub fn document_dir(root: &Path, configured: &str) -> Result<PathBuf> {
    let rel = Path::new(configured);
    if rel.is_absolute() {
        return Err(Error::InvalidStore(format!(
            "document directory '{configured}' must be relative to the store"
        )));
    }
    if escapes_root(rel) {
        return Err(Error::InvalidStore(format!(
            "document directory '{configured}' leaves the store root"
        )));
    }
    let joined = root.join(rel);
    // The text passing the check is not enough. The directory itself
    // can be a symlink, and the store is third-party content, so the
    // resolved location must sit inside the resolved root.
    if let (Ok(real_root), Ok(real_dir)) = (root.canonicalize(), joined.canonicalize())
        && !real_dir.starts_with(&real_root)
    {
        return Err(Error::InvalidStore(format!(
            "document directory '{configured}' resolves outside the store root"
        )));
    }
    Ok(joined)
}

/// Fetch the current state of a remote source into the cache.
///
/// Only a command that the user expects to reach the network calls
/// this. Every other command reads what the cache already holds, so an
/// answer never changes because of a background fetch.
pub fn sync_source(source: &StoreSource) -> Result<()> {
    match source {
        StoreSource::Path(_) => Ok(()),
        StoreSource::Git { url, rev } => {
            crate::git::fetch(url)?;
            // A fetch that moved bytes is not a sync. The declared pin
            // has to resolve in what arrived, or the report says
            // synced and the pin fails later, on read, as a
            // gix-internal message. QA hit exactly that on 2026-08-16.
            let dir = crate::git::cache_dir(url);
            match crate::git::resolve_rev(&dir, rev.as_deref()) {
                Ok(_) => Ok(()),
                Err(_) => match rev {
                    Some(pin) => Err(Error::InvalidStore(format!("pin {pin} not found in {url}"))),
                    // The fetch succeeded and HEAD still does not
                    // resolve. Usually the source has no commits; the
                    // reviewer also built a HEAD naming a deleted
                    // branch while other branches held commits, so the
                    // message states what is known and names both
                    // causes rather than asserting the common one.
                    None => Err(Error::InvalidStore(format!(
                        "HEAD does not resolve in {url}: the source has no commits, or HEAD names a deleted branch"
                    ))),
                },
            }
        }
        StoreSource::Blob { url } => crate::blob::sync(url).map(|_| ()),
    }
}

/// True when the text names one document, not a path.
///
/// An id becomes a file path when a store joins it onto a document
/// directory, so it must hold no separator, no parent component, and
/// no root. A served store takes this text from the network.
///
/// One predicate serves every tool. A guard that lives in one of two
/// sibling tools is a guard that is missing from the other.
pub fn is_plain_stem(input: &str) -> bool {
    !input.is_empty()
        && !input.contains('/')
        && !input.contains('\\')
        && input != "."
        && input != ".."
        && !input.starts_with('.')
        && !input.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Aliases(Vec<String>);
    impl AliasSet for Aliases {
        fn declares(&self, alias: &str) -> bool {
            self.0.iter().any(|a| a == alias)
        }
    }

    fn aliases(list: &[&str]) -> Aliases {
        Aliases(list.iter().map(|s| s.to_string()).collect())
    }

    /// An @ in a local path is not scp syntax.
    ///
    /// canonical_url read any scheme-less @ as user@host, kept what
    /// followed, and lowercased it. /x/at1/x@1 and /x/at2/y@1 both
    /// became "1" and shared one cache slot, and two local paths
    /// differing by case merged on a case-sensitive filesystem.
    #[test]
    fn a_local_path_with_an_at_sign_is_not_scp_form() {
        // Two different local declarations stay two slots.
        let a = canonical_url_for_cache("/x/at1/x@1");
        let b = canonical_url_for_cache("/x/at2/y@1");
        assert_ne!(a, b, "two local paths with @ share a slot key");
        assert_eq!(a, "/x/at1/x@1", "a local path was rewritten: {a}");

        // A relative declaration too.
        assert_eq!(canonical_url_for_cache("../up@2"), "../up@2");

        // Real scp form still folds onto its https spelling.
        assert_eq!(
            canonical_url_for_cache("git@github.com:Owner/Repo.git"),
            "github.com/owner/repo"
        );
        assert_eq!(
            canonical_url_for_cache("https://GitHub.com/Owner/Repo/"),
            "github.com/owner/repo"
        );

        // Case distinguishes local repositories.
        assert_ne!(
            canonical_url_for_cache("/p/Case"),
            canonical_url_for_cache("/p/case"),
            "case-different local paths merged"
        );

        // scp form has a colon: user@host:path. Without one, an @ in
        // the first path segment still collapsed: up@2 and down@2
        // both keyed 2, and alpha@2/kb and beta@2/kb both keyed 2/kb.
        assert_ne!(
            canonical_url_for_cache("up@2"),
            canonical_url_for_cache("down@2"),
            "two first-segment @ names share a slot key"
        );
        assert_ne!(
            canonical_url_for_cache("alpha@2/kb"),
            canonical_url_for_cache("beta@2/kb"),
            "two @-segment paths share a slot key"
        );
        assert_eq!(canonical_url_for_cache("up@2"), "up@2");
    }

    // -- references --

    #[test]
    fn bare_id_has_no_alias() {
        let r = StoreRef::parse("a3f2", &NoAliases);
        assert_eq!(r, StoreRef::local("a3f2"));
        assert!(!r.is_foreign());
    }

    #[test]
    fn declared_alias_qualifies_the_reference() {
        let r = StoreRef::parse("project:a3f2", &aliases(&["project"]));
        assert_eq!(r.alias, vec!["project".to_string()]);
        assert_eq!(r.id, "a3f2");
        assert!(r.is_foreign());
    }

    #[test]
    fn undeclared_head_stays_opaque() {
        // The discrimination rule: a colon alone means nothing.
        let r = StoreRef::parse("project:a3f2", &NoAliases);
        assert_eq!(r, StoreRef::local("project:a3f2"));
    }

    #[test]
    fn external_keys_keep_working() {
        // A DOI, a legacy tisket depends_on entry, and a URL must not
        // become references against a normal alias table.
        for s in ["10.1145/12345", "x: y", "https://example.com/a", "RFC:8259"] {
            let r = StoreRef::parse(s, &aliases(&["project", "org-kb"]));
            assert_eq!(r, StoreRef::local(s), "{s} must stay opaque");
        }
    }

    #[test]
    fn a_declared_alias_wins_over_an_external_reading() {
        // The rule is declaration, not shape: declaring 'doi' makes
        // doi:x a reference. This is correct behavior, and it is also
        // why `check` reports the qualifiers that a new alias shadows.
        let r = StoreRef::parse("doi:10.1145/1", &aliases(&["doi"]));
        assert!(r.is_foreign());
        assert_eq!(r.id, "10.1145/1");
    }

    #[test]
    fn only_the_first_colon_splits() {
        let r = StoreRef::parse("project:a:b", &aliases(&["project"]));
        assert_eq!(r.alias, vec!["project".to_string()]);
        assert_eq!(r.id, "a:b");
    }

    #[test]
    fn path_form_parses_when_the_head_is_declared() {
        let r = StoreRef::parse("project/shared-kb:b7c1", &aliases(&["project"]));
        assert_eq!(
            r.alias,
            vec!["project".to_string(), "shared-kb".to_string()]
        );
        assert_eq!(r.id, "b7c1");
    }

    #[test]
    fn path_form_with_undeclared_head_is_opaque() {
        let r = StoreRef::parse("nope/shared-kb:b7c1", &aliases(&["project"]));
        assert!(!r.is_foreign());
    }

    #[test]
    fn empty_id_does_not_qualify() {
        let r = StoreRef::parse("project:", &aliases(&["project"]));
        assert_eq!(r, StoreRef::local("project:"));
    }

    #[test]
    fn references_round_trip_through_display() {
        for (s, decl) in [
            ("a3f2", vec![]),
            ("project:a3f2", vec!["project"]),
            ("project/shared-kb:b7c1", vec!["project"]),
        ] {
            let r = StoreRef::parse(s, &aliases(&decl));
            assert_eq!(r.to_string(), s);
        }
    }

    // -- identity --

    #[test]
    fn urls_written_differently_share_an_identity() {
        let a = StoreSource::Git {
            url: "https://github.com/org/kb.git".into(),
            rev: None,
        };
        let b = StoreSource::Git {
            url: "git@github.com:org/kb".into(),
            rev: None,
        };
        assert_eq!(a.identity(None, &NoOrigins), b.identity(None, &NoOrigins));
    }

    #[test]
    fn a_rev_pin_does_not_change_identity() {
        let a = StoreSource::Git {
            url: "https://github.com/org/kb".into(),
            rev: Some("v1.0".into()),
        };
        let b = StoreSource::Git {
            url: "https://github.com/org/kb".into(),
            rev: None,
        };
        assert_eq!(a.identity(None, &NoOrigins), b.identity(None, &NoOrigins));
    }

    #[test]
    fn a_path_takes_its_origin_remote_identity() {
        struct WithOrigin;
        impl OriginLookup for WithOrigin {
            fn origin_url(&self, _p: &Path) -> Option<String> {
                Some("https://github.com/org/kb".into())
            }
        }
        let dir = tempdir();
        let by_path = StoreSource::Path(dir.clone());
        let by_url = StoreSource::Git {
            url: "git@github.com:org/kb.git".into(),
            rev: None,
        };
        assert_eq!(
            by_path.identity(Some(&dir), &WithOrigin),
            by_url.identity(None, &NoOrigins),
            "the same repo reached two ways is one store"
        );
    }

    #[test]
    fn identity_comes_from_the_resolved_location_not_the_declared_text() {
        // '../b' in two stores names two directories. One directory
        // that two different relative paths find is one store.
        let base = tempdir();
        std::fs::create_dir_all(base.join("shared")).unwrap();
        let decl_a = StoreSource::Path("../shared".into());
        let decl_b = StoreSource::Path("./shared".into());
        let located = base.join("shared");
        assert_eq!(
            decl_a.identity(Some(&located), &NoOrigins),
            decl_b.identity(Some(&located), &NoOrigins)
        );

        let other = base.join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert_ne!(
            decl_a.identity(Some(&located), &NoOrigins),
            decl_a.identity(Some(&other), &NoOrigins),
            "same declared text, different locations, different stores"
        );
    }

    // -- config --

    #[test]
    fn a_missing_stores_file_is_an_empty_config() {
        let dir = tempdir();
        let config = StoresConfig::load(&dir).unwrap();
        assert!(config.stores.is_empty());
        assert_eq!(config.format, None);
    }

    #[test]
    fn declarations_parse_with_their_sources() {
        let dir = tempdir();
        write(
            &dir.join(STORES_FILE),
            "format: 2\nstores:\n  - alias: project\n    path: ../project\n  - alias: org-kb\n    git: https://github.com/org/kb\n    rev: v1.0\n",
        );
        let config = StoresConfig::load(&dir).unwrap();
        assert_eq!(config.aliases(), vec!["project", "org-kb"]);
        assert_eq!(
            config.source("project"),
            Some(&StoreSource::Path("../project".into()))
        );
        assert!(matches!(
            config.source("org-kb"),
            Some(StoreSource::Git { rev: Some(r), .. }) if r == "v1.0"
        ));
    }

    #[test]
    fn a_source_needs_exactly_one_kind() {
        let dir = tempdir();
        for yaml in [
            "stores:\n  - alias: a\n",
            "stores:\n  - alias: a\n    path: ../x\n    git: https://e.com/x\n",
            "stores:\n  - alias: a\n    path: ../x\n    rev: v1\n",
        ] {
            write(&dir.join(STORES_FILE), yaml);
            assert!(StoresConfig::load(&dir).is_err(), "must reject: {yaml}");
        }
    }

    #[test]
    fn a_newer_format_is_rejected() {
        let dir = tempdir();
        write(&dir.join(STORES_FILE), "format: 99\nstores: []\n");
        let err = StoresConfig::load(&dir).unwrap_err().to_string();
        assert!(err.contains("99"), "{err}");
    }

    #[test]
    fn a_duplicate_alias_is_rejected() {
        let dir = tempdir();
        write(
            &dir.join(STORES_FILE),
            "stores:\n  - alias: a\n    path: ../x\n  - alias: a\n    path: ../y\n",
        );
        assert!(StoresConfig::load(&dir).is_err());
    }

    #[test]
    fn shared_stores_may_not_declare_paths_that_leave_the_repo() {
        let config = StoresConfig {
            format: Some(2),
            name: None,
            shared: true,
            stores: vec![
                StoreDecl {
                    alias: "inside".into(),
                    source: StoreSource::Path("sub/kb".into()),
                },
                StoreDecl {
                    alias: "sibling".into(),
                    source: StoreSource::Path("../sibling".into()),
                },
                StoreDecl {
                    alias: "home".into(),
                    source: StoreSource::Path("~/kb".into()),
                },
                StoreDecl {
                    alias: "abs".into(),
                    source: StoreSource::Path("/srv/kb".into()),
                },
            ],
        };
        let bad: Vec<String> = config
            .unshareable(Path::new("/repo"))
            .into_iter()
            .map(|(a, _)| a)
            .collect();
        assert_eq!(bad, vec!["sibling", "home", "abs"]);

        // A private store may point anywhere: nobody else resolves it.
        let private = StoresConfig {
            shared: false,
            ..config
        };
        assert!(private.unshareable(Path::new("/repo")).is_empty());
    }

    // -- the graph --

    fn store_at(dir: &Path, yaml: &str) {
        std::fs::create_dir_all(dir).unwrap();
        write(&dir.join(STORES_FILE), yaml);
    }

    #[test]
    fn a_chain_yields_every_member_once() {
        let base = tempdir();
        store_at(&base.join("a"), "stores:\n  - alias: b\n    path: ../b\n");
        store_at(&base.join("b"), "stores:\n  - alias: c\n    path: ../c\n");
        store_at(&base.join("c"), "stores: []\n");
        let graph = StoreGraph::open(&base.join("a"), &LocalPaths).unwrap();
        let paths: Vec<String> = graph
            .members
            .iter()
            .map(|m| m.alias_path.join("/"))
            .collect();
        assert_eq!(paths, vec!["", "b", "b/c"]);
    }

    #[test]
    fn a_mutual_cycle_terminates() {
        let base = tempdir();
        store_at(&base.join("a"), "stores:\n  - alias: b\n    path: ../b\n");
        store_at(&base.join("b"), "stores:\n  - alias: a\n    path: ../a\n");
        let graph = StoreGraph::open(&base.join("a"), &LocalPaths).unwrap();
        assert_eq!(graph.members.len(), 2, "each store once");
    }

    #[test]
    fn a_diamond_yields_the_shared_store_once() {
        let base = tempdir();
        store_at(
            &base.join("a"),
            "stores:\n  - alias: b\n    path: ../b\n  - alias: c\n    path: ../c\n",
        );
        store_at(&base.join("b"), "stores:\n  - alias: d\n    path: ../d\n");
        store_at(&base.join("c"), "stores:\n  - alias: d\n    path: ../d\n");
        store_at(&base.join("d"), "stores: []\n");
        let graph = StoreGraph::open(&base.join("a"), &LocalPaths).unwrap();
        assert_eq!(graph.members.len(), 4);
        let d: Vec<&Member> = graph
            .members
            .iter()
            .filter(|m| m.alias_path.last().map(|s| s.as_str()) == Some("d"))
            .collect();
        assert_eq!(d.len(), 1, "the shared store is one member");
        assert_eq!(d[0].alias_path, vec!["b".to_string(), "d".to_string()]);
    }

    #[test]
    fn an_alias_resolves_through_the_declaring_store() {
        // The confused-deputy case: root and dep both declare 'kb', at
        // different stores. A reference in the dep must reach the dep's.
        let base = tempdir();
        store_at(
            &base.join("root"),
            "stores:\n  - alias: dep\n    path: ../dep\n  - alias: kb\n    path: ../root-kb\n",
        );
        store_at(
            &base.join("dep"),
            "stores:\n  - alias: kb\n    path: ../dep-kb\n",
        );
        store_at(&base.join("root-kb"), "stores: []\n");
        store_at(&base.join("dep-kb"), "stores: []\n");
        let graph = StoreGraph::open(&base.join("root"), &LocalPaths).unwrap();

        let dep = graph
            .members
            .iter()
            .position(|m| m.alias_path == vec!["dep".to_string()])
            .unwrap();
        let from_root = graph.target_of(0, "kb").unwrap();
        let from_dep = graph.target_of(dep, "kb").unwrap();
        assert_ne!(
            from_root, from_dep,
            "the same alias names different stores in different configs"
        );
        assert!(
            graph.members[from_root]
                .content
                .as_ref()
                .unwrap()
                .dir()
                .unwrap()
                .ends_with("root-kb")
        );
        assert!(
            graph.members[from_dep]
                .content
                .as_ref()
                .unwrap()
                .dir()
                .unwrap()
                .ends_with("dep-kb")
        );
    }

    #[test]
    fn a_missing_dependency_is_reported_not_fatal() {
        let base = tempdir();
        store_at(
            &base.join("a"),
            "stores:\n  - alias: gone\n    path: ../gone\n",
        );
        let graph = StoreGraph::open(&base.join("a"), &LocalPaths).unwrap();
        assert_eq!(graph.members.len(), 2);
        assert_eq!(graph.unavailable().len(), 1);
        assert!(graph.findings.iter().any(|f| f.contains("gone")));
    }

    #[test]
    fn members_qualify_their_documents() {
        let base = tempdir();
        store_at(&base.join("a"), "stores:\n  - alias: b\n    path: ../b\n");
        store_at(&base.join("b"), "stores:\n  - alias: c\n    path: ../c\n");
        store_at(&base.join("c"), "stores: []\n");
        let graph = StoreGraph::open(&base.join("a"), &LocalPaths).unwrap();
        assert_eq!(graph.members[0].qualify("a3f2"), "a3f2");
        assert_eq!(graph.members[1].qualify("a3f2"), "b:a3f2");
        assert_eq!(graph.members[2].qualify("a3f2"), "b/c:a3f2");
    }

    // -- guards --

    #[test]
    fn store_content_refuses_a_symlinked_file() {
        // A consumer that reads through StoreContent must not be able
        // to follow a link out of the store.
        let base = tempdir();
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        let outside = base.join("outside.md");
        write(&outside, "PRIVATE");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, store.join("linked.md")).unwrap();

        let content = StoreContent::Dir(crate::confined::StoreDir::open(&store).unwrap());
        assert!(!content.exists("linked.md"), "a link is not a file here");
        assert!(content.read("linked.md").is_err());
    }

    #[test]
    fn a_symlinked_document_is_not_read() {
        let dir = tempdir();
        let secret = dir.join("secret.txt");
        write(&secret, "PRIVATE");
        let link = dir.join("note.md");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let store = crate::confined::StoreDir::open(&dir).unwrap();
        let err = store.read("note.md").unwrap_err().to_string();
        assert!(err.contains("symlink"), "{err}");
        assert!(!store.is_document("note.md"));
    }

    #[test]
    fn a_regular_document_is_read() {
        let dir = tempdir();
        let path = dir.join("note.md");
        write(&path, "---\ntitle: t\n---\n");
        let store = crate::confined::StoreDir::open(&dir).unwrap();
        assert!(store.is_document("note.md"));
        assert!(store.read("note.md").unwrap().contains("title: t"));
    }

    #[test]
    fn a_write_replaces_a_symlink_instead_of_its_target() {
        let dir = tempdir();
        let target = dir.join("outside.md");
        write(&target, "ORIGINAL");
        let link = dir.join("note.md");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let store = crate::confined::StoreDir::open(&dir).unwrap();
        store.write("note.md", "REPLACED").unwrap();

        // The link is gone, and what it pointed at is untouched.
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "ORIGINAL");
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "REPLACED");
        assert!(store.is_document("note.md"));
    }

    #[test]
    fn a_symlinked_document_directory_is_refused() {
        // The configured text passes the check, and the directory it
        // names is a link out of the store.
        let base = tempdir();
        let store = base.join("store");
        let outside = base.join("outside");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, store.join(".zettel")).unwrap();
        let err = document_dir(&store, ".zettel").unwrap_err().to_string();
        assert!(err.contains("outside the store root"), "{err}");
    }

    #[test]
    fn a_real_document_directory_is_allowed() {
        let base = tempdir();
        let store = base.join("store");
        std::fs::create_dir_all(store.join(".zettel")).unwrap();
        assert!(document_dir(&store, ".zettel").is_ok());
    }

    #[test]
    fn a_vendored_dependency_may_not_name_this_machine_under_any_key() {
        // Drives the walk, not the helpers. The helper-only version of
        // this test passed while the guard at the call site still
        // narrowed to StoreSource::Path, so the defect it describes
        // was reported fixed and was not.
        //
        // A vendored third-party store is a plain local directory. Its
        // own declarations must not name a location the reader chose
        // for themselves.
        let base = tempdir();
        let private = base.join("private");
        store_at(&private, "stores: []\n");
        let abs = private.display().to_string();

        for (label, decl) in [
            ("path", format!("path: {abs}")),
            ("git-abs", format!("git: {abs}")),
            ("git-file-url", format!("git: file://{abs}")),
            ("git-file-url-upper", format!("git: FILE://{abs}")),
            ("git-file-url-host", format!("git: file://localhost{abs}")),
            ("blob-abs", format!("blob: {abs}")),
        ] {
            let root = base.join(format!("root-{label}"));
            let vendored = base.join(format!("vendored-{label}"));
            store_at(&vendored, &format!("stores:\n  - alias: pwn\n    {decl}\n"));
            store_at(
                &root,
                &format!("stores:\n  - alias: dep\n    path: ../vendored-{label}\n"),
            );

            let graph = StoreGraph::open(&root, &LocalPaths).unwrap();
            assert!(
                !graph
                    .members
                    .iter()
                    .any(|m| m.alias_path == vec!["dep".to_string(), "pwn".to_string()]),
                "{label}: the private store entered the closure"
            );
            assert!(
                graph.findings.iter().any(|f| f.contains("dep/pwn")),
                "{label}: no finding reported; findings were {:?}",
                graph.findings
            );
        }
    }

    #[test]
    fn a_remote_store_may_not_declare_a_local_path() {
        // Publishing a store would otherwise be enough to pull a
        // reader's own directories into their closure: the remote store
        // declares `path: /anything` and the reader loads it.
        struct GitResolvesToDir(PathBuf);
        impl SourceLocator for GitResolvesToDir {
            fn locate(
                &self,
                source: &StoreSource,
                declaring_root: &Path,
            ) -> std::result::Result<StoreContent, String> {
                match source {
                    // Stand in for a fetched clone.
                    StoreSource::Git { .. } => crate::confined::StoreDir::open(&self.0)
                        .map(StoreContent::Dir)
                        .map_err(|e| e.to_string()),
                    other => LocalPaths.locate(other, declaring_root),
                }
            }
        }

        let base = tempdir();
        let upstream = base.join("upstream");
        let private = base.join("private");
        store_at(&private, "stores: []\n");
        // The remote store declares the reader's private directory.
        store_at(
            &upstream,
            &format!("stores:\n  - alias: pwn\n    path: {}\n", private.display()),
        );
        store_at(
            &base.join("root"),
            "stores:\n  - alias: kb\n    git: https://example.com/org/kb\n",
        );

        let graph =
            StoreGraph::open(&base.join("root"), &GitResolvesToDir(upstream.clone())).unwrap();

        let kb = graph
            .members
            .iter()
            .find(|m| m.alias_path == vec!["kb".to_string()])
            .expect("the remote store is a member");
        assert!(kb.remote, "a git source marks the member remote");

        assert!(
            !graph
                .members
                .iter()
                .any(|m| m.alias_path == vec!["kb".to_string(), "pwn".to_string()]),
            "the private directory must not enter the closure"
        );
        assert!(
            graph
                .findings
                .iter()
                .any(|f| f.contains("may not declare a location on this machine")),
            "the refusal is reported: {:?}",
            graph.findings
        );
    }

    #[test]
    fn a_symlink_at_the_staging_path_cannot_capture_a_write() {
        let base = std::env::temp_dir().join(format!(
            "mdstore-staging-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("store")).unwrap();
        let victim = base.join("victim.txt");
        std::fs::write(&victim, "untouched").unwrap();
        let doc = base.join("store").join("note.md");
        std::fs::write(&doc, "original").unwrap();

        // Every staging name this process could pick, planted as a link.
        for n in 0..8 {
            let link = base
                .join("store")
                .join(format!(".note.md.{}.{n}.tmp", std::process::id()));
            let _ = std::os::unix::fs::symlink(&victim, &link);
        }

        // The write either refuses or stages elsewhere. Either way the
        // victim keeps its content and the document is never a link.
        let store = crate::confined::StoreDir::open(&base.join("store")).unwrap();
        let _ = store.write("note.md", "new content");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "untouched");
        assert!(
            !std::fs::symlink_metadata(&doc)
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_shared_store_may_not_reach_outside_through_a_link() {
        let base = std::env::temp_dir().join(format!("mdstore-shared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("repo")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::os::unix::fs::symlink(base.join("outside"), base.join("repo").join("linked")).unwrap();
        std::fs::create_dir_all(base.join("repo").join("inside")).unwrap();

        let config: StoresConfig = yaml_serde::from_str(
            "shared: true\nstores:\n  - alias: near\n    path: inside\n  \
             - alias: far\n    path: linked\n",
        )
        .unwrap();
        let bad = config.unshareable(&base.join("repo"));
        let aliases: Vec<&str> = bad.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(aliases, vec!["far"], "{bad:?}");
        assert!(bad[0].1.contains("through a link"), "{bad:?}");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_stores_file_that_is_a_link_is_refused_not_ignored() {
        let base = std::env::temp_dir().join(format!("mdstore-storesfile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("store")).unwrap();
        std::fs::write(base.join("elsewhere.yml"), "stores: []\n").unwrap();
        std::os::unix::fs::symlink(
            base.join("elsewhere.yml"),
            base.join("store").join(STORES_FILE),
        )
        .unwrap();

        let err = StoresConfig::load(&base.join("store")).unwrap_err();
        assert!(format!("{err}").contains("not a regular file"), "{err}");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn every_spelling_of_this_machine_is_local() {
        // The URL parser that performs the fetch resolves all of these
        // to 127.0.0.1. The hand-written test called them remote.
        for spelling in [
            "http://127.1/kb",
            "http://2130706433/kb",
            "http://0x7f000001/kb",
            "http://0177.0.0.1/kb",
            "http://127.0.0.1./kb",
        ] {
            assert!(!is_remote_url(spelling), "{spelling} names this machine");
        }
        // A connection to the unspecified address reaches a local
        // listener, and is_loopback answers false for it.
        for spelling in ["http://0.0.0.0/kb", "http://[::]/kb"] {
            assert!(!is_remote_url(spelling), "{spelling} names this machine");
        }
        // The IPv4-mapped form reaches the same service as the plain one.
        assert!(!is_remote_url("http://[::ffff:127.0.0.1]/kb"));

        // A real host stays remote.
        assert!(is_remote_url("https://example.com/kb"));
        assert!(is_remote_url("https://127.0.0.1.example.com/kb"));
        assert!(is_remote_url("https://8.8.8.8/kb"));
    }

    #[test]
    fn a_symlinked_subdirectory_is_skipped() {
        let base = std::env::temp_dir().join(format!("mdstore-subdirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("store/docs/real")).unwrap();
        std::fs::create_dir_all(base.join("outside/secret")).unwrap();
        std::os::unix::fs::symlink(base.join("outside"), base.join("store/docs/linked")).unwrap();
        std::fs::create_dir_all(base.join("store/docs/.hidden")).unwrap();

        let content =
            StoreContent::Dir(crate::confined::StoreDir::open(&base.join("store")).unwrap());
        assert_eq!(content.subdirectories("docs"), vec!["real".to_string()]);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn two_local_git_sources_are_two_members() {
        // `git: ../shared` in two different stores names two different
        // directories. The declared text keyed both, so the second
        // silently answered with the first one's documents.
        let base = std::env::temp_dir().join(format!("mdstore-localgit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("a")).unwrap();
        std::fs::create_dir_all(base.join("b")).unwrap();

        let a = StoreSource::Git {
            url: "../shared".into(),
            rev: None,
        };
        let b = StoreSource::Git {
            url: "../shared".into(),
            rev: None,
        };
        assert_ne!(
            a.identity(Some(&base.join("a")), &NoOrigins),
            b.identity(Some(&base.join("b")), &NoOrigins),
            "two directories are two members"
        );
        // One directory reached twice stays one member.
        assert_eq!(
            a.identity(Some(&base.join("a")), &NoOrigins),
            b.identity(Some(&base.join("a")), &NoOrigins)
        );
        // A real URL still identifies the store wherever it is read.
        let remote = StoreSource::Git {
            url: "https://example.com/org/kb".into(),
            rev: None,
        };
        assert_eq!(
            remote.identity(Some(&base.join("a")), &NoOrigins),
            remote.identity(Some(&base.join("b")), &NoOrigins)
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_local_location_is_refused_under_every_key() {
        // The round-two critical, reopened at the git:/blob: call site:
        // a dependency naming an absolute path passes when the guard
        // narrows to StoreSource::Path first.
        for source in [
            StoreSource::Path("/Users/someone/private".into()),
            StoreSource::Git {
                url: "/Users/someone/private".into(),
                rev: None,
            },
            StoreSource::Blob {
                url: "file:///Users/someone/private".into(),
            },
        ] {
            let located = on_machine_location(&source).expect("names this machine");
            assert!(
                anchored_to_one_machine(&located).is_some(),
                "{source:?} must be refused"
            );
        }

        // A relative path travels with a copy of the working area.
        assert!(
            on_machine_location(&StoreSource::Path("../sibling".into()))
                .is_some_and(|p| anchored_to_one_machine(&p).is_none())
        );
        // A real URL names no location here.
        assert!(
            on_machine_location(&StoreSource::Git {
                url: "https://example.com/org/kb".into(),
                rev: None
            })
            .is_none()
        );
    }

    #[test]
    fn a_loopback_url_is_not_a_remote_location() {
        // The scheme said remote; the host is this machine.
        assert!(!is_remote_url("http://127.0.0.1/kb"));
        assert!(!is_remote_url("http://localhost:8080/kb"));
        assert!(!is_remote_url("https://LOCALHOST/kb"));
        assert!(!is_remote_url("http://[::1]:9000/kb"));
        assert!(!is_remote_url("http://localhost./kb"));
        assert!(!is_remote_url("git@localhost:org/kb"));
        assert!(!is_remote_url("ssh://git@127.0.0.1/org/kb"));

        // Userinfo does not smuggle a host past the check.
        assert!(!is_remote_url("https://example.com@127.0.0.1/kb"));
        assert!(is_remote_url("https://127.0.0.1.example.com/kb"));
        assert!(is_remote_url("https://example.com/kb"));
    }

    #[test]
    fn a_dependency_may_not_declare_a_path_anchored_to_one_machine() {
        assert_eq!(anchored_to_one_machine(Path::new("../sibling")), None);
        assert_eq!(anchored_to_one_machine(Path::new("nested/store")), None);
        assert!(anchored_to_one_machine(Path::new("/Users/someone/private")).is_some());
        assert!(anchored_to_one_machine(Path::new("~/private")).is_some());
    }

    #[test]
    fn a_shared_store_may_not_hide_a_local_path_under_git_or_blob() {
        let config: StoresConfig = yaml_serde::from_str(
            "shared: true\nstores:\n  \
             - alias: ok\n    git: https://example.com/org/kb\n  \
             - alias: sneaky\n    git: /Users/someone/private\n  \
             - alias: filed\n    blob: file:///Users/someone/private\n",
        )
        .unwrap();
        let bad = config.unshareable(Path::new("/repo"));
        let aliases: Vec<&str> = bad.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(aliases, vec!["sneaky", "filed"], "{bad:?}");
    }

    #[test]
    fn a_plain_stem_is_one_document_and_never_a_path() {
        assert!(is_plain_stem("ab12-fix-the-widget"));
        assert!(is_plain_stem("a3f2"));

        assert!(!is_plain_stem(""));
        assert!(!is_plain_stem("."));
        assert!(!is_plain_stem(".."));
        assert!(!is_plain_stem("../../outside"));
        assert!(!is_plain_stem("/etc/passwd"));
        assert!(!is_plain_stem("sub/note"));
        assert!(!is_plain_stem("sub\\note"));
        assert!(!is_plain_stem(".hidden"));
        assert!(!is_plain_stem("with\0nul"));
    }

    #[test]
    fn a_remote_location_is_judged_by_the_value_not_the_key() {
        // What a stranger may declare.
        assert!(is_remote_url("https://example.com/org/kb"));
        assert!(is_remote_url("ssh://git@example.com/org/kb"));
        assert!(is_remote_url("git@example.com:org/kb"));
        assert!(is_remote_url("s3://bucket/notes"));

        // What a stranger may not: every spelling of this machine.
        assert!(!is_remote_url("/Users/someone/private-kb"));
        assert!(!is_remote_url("../private"));
        assert!(!is_remote_url("private"));
        assert!(!is_remote_url("file:///Users/someone/private-kb"));
        assert!(!is_remote_url("FILE:///Users/someone/private-kb"));
        assert!(!is_remote_url("C:/Users/someone/private-kb"));
        assert!(!is_remote_url(""));

        // The variant proves nothing; the value decides.
        assert!(!declares_a_remote_location(&StoreSource::Git {
            url: "/Users/someone/private-kb".into(),
            rev: None
        }));
        assert!(!declares_a_remote_location(&StoreSource::Blob {
            url: "../private".into()
        }));
        assert!(declares_a_remote_location(&StoreSource::Git {
            url: "https://example.com/org/kb".into(),
            rev: None
        }));
        assert!(!declares_a_remote_location(&StoreSource::Path(
            "../anything".into()
        )));
    }

    #[test]
    fn a_document_dir_may_not_escape_the_store() {
        assert!(document_dir(Path::new("/store"), ".zettel").is_ok());
        assert!(document_dir(Path::new("/store"), "sub/.zettel").is_ok());
        assert!(document_dir(Path::new("/store"), "/etc").is_err());
        assert!(document_dir(Path::new("/store"), "../../etc").is_err());
        assert!(document_dir(Path::new("/store"), "a/../../etc").is_err());
    }

    #[test]
    fn a_symlinked_document_is_skipped_and_reported() {
        let dir = tempdir();
        write(&dir.join("real.md"), "---\ntitle: t\n---\n");
        let secret = dir.join("secret.txt");
        write(&secret, "PRIVATE KEY");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&secret, dir.join("stolen.md")).unwrap();

        let scan = crate::confined::StoreDir::open(&dir)
            .unwrap()
            .scan("")
            .unwrap();
        let stems: Vec<&str> = scan.entries.iter().map(|e| e.stem.as_str()).collect();
        assert_eq!(stems, vec!["real"], "only the real file is a document");
        assert_eq!(scan.skipped.len(), 1);
        assert!(scan.skipped[0].1.contains("symlink"));
    }

    #[test]
    fn a_scan_of_a_missing_directory_is_empty() {
        let store = crate::confined::StoreDir::open(&tempdir()).unwrap();
        let scan = store.scan("nope").unwrap();
        assert!(scan.entries.is_empty());
        assert!(scan.skipped.is_empty());
    }

    // -- helpers --

    fn tempdir() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "mdstore-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }

    /// The claim the whole capability exists to make.
    ///
    /// Every one of these returned the outside file before
    /// StoreContent held a handle. The module was present and no
    /// caller went through it.
    #[test]
    fn a_store_read_cannot_leave_the_store() {
        let base = std::env::temp_dir().join(format!("mdstore-escape-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        write(&base.join("outside/secret.md"), "SECRET");
        write(&base.join("store/docs/real.md"), "REAL");
        let content =
            StoreContent::Dir(crate::confined::StoreDir::open(&base.join("store")).unwrap());

        assert!(
            content.read("../outside/secret.md").is_err(),
            "a climbing read escaped"
        );
        assert!(
            !content.exists("../outside/secret.md"),
            "a climbing path was reported present"
        );
        assert!(
            content.read("/etc/hosts").is_err(),
            "an absolute read escaped"
        );
        assert!(
            !content.present_but_irregular("/etc"),
            "an absolute path answered for a directory outside the store"
        );
        // An escape is refused where the signature can carry a
        // refusal, and is empty where it cannot. A missing directory
        // inside the store stays Ok(empty); only leaving is an error.
        assert!(
            content.scan("../outside").is_err(),
            "a climbing scan listed documents outside the store"
        );
        assert!(
            content.scan("nosuchdir").unwrap().entries.is_empty(),
            "a missing directory inside the store must not be an error"
        );
        // A climb through a directory that does not exist. The
        // operating system answers NotFound before it evaluates the
        // climb, and the missing-directory arm swallowed the refusal.
        assert!(
            content
                .scan("docs/a/b/../../../../outside/secret.md")
                .is_err(),
            "a climb through a missing directory was answered empty"
        );
        assert!(
            !content.subdirectories("..").iter().any(|n| n == "outside"),
            "a climbing listing named a sibling of the store"
        );

        // The store still works for what is genuinely inside it.
        assert_eq!(content.read("docs/real.md").unwrap(), "REAL");
        assert_eq!(content.scan("docs").unwrap().entries.len(), 1);

        let _ = std::fs::remove_dir_all(&base);
    }
}
