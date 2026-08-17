//! A read-only git cache for remote stores, on gix. Nothing here
//! spawns a process.
//!
//! The cache holds one bare clone for each URL. Documents are read from
//! git objects at the revision that each consumer declares. A bare clone
//! has no working tree, so two consumers that pin different revisions of
//! one URL share the same fetch and cannot overwrite each other. A
//! working tree would give the last consumer that synchronized control
//! of what every other consumer reads.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use gix::bstr::ByteSlice as _;

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

/// Where a declared URL leads, and whether gix can reach it in-process.
enum Source {
    /// A repository on this machine: `file://…`, an absolute path, or a
    /// relative path. gix's file transport would spawn `git-upload-pack`,
    /// so the slot is filled by reading the source repository directly.
    Local(PathBuf),
    /// http, https, git://. gix speaks these in-process.
    Network(gix::Url),
    /// ssh:// or scp form. gix has no in-process ssh transport; it would
    /// spawn `ssh`. Refused, with the fix in the message.
    Ssh,
}

/// The location on this machine that a declared source names, if it
/// names one.
///
/// This asks the parser that performs the fetch, so the answer is what
/// gix will actually read. A guard that decides this any other way
/// decides a different question: `strip_prefix("file://")` misses
/// `FILE://` and misreads `file://localhost/abs/path`, both of which
/// gix resolves to `/abs/path`.
///
/// `None` means the source names no location on this machine.
#[must_use]
pub fn local_path(url: &str) -> Option<PathBuf> {
    match classify(url) {
        Ok(Source::Local(p)) => Some(p),
        // A source that does not parse names nothing this machine can
        // reach, and a caller that must decide safety treats an
        // unparseable declaration as unresolved rather than as remote.
        Ok(_) | Err(_) => None,
    }
}

fn classify(url: &str) -> Result<Source> {
    let parsed = gix::url::parse(gix::bstr::BStr::new(url))
        .map_err(|e| Error::InvalidStore(format!("{url}: {e}")))?;
    Ok(match parsed.scheme {
        gix::url::Scheme::File => {
            // A relative path resolves against the process cwd, as the
            // git CLI clone did. The slot is keyed by the declared text.
            let p = gix::path::from_bstr(parsed.path.as_bstr()).into_owned();
            Source::Local(std::path::absolute(&p).unwrap_or(p))
        }
        gix::url::Scheme::Ssh => Source::Ssh,
        gix::url::Scheme::Http | gix::url::Scheme::Https | gix::url::Scheme::Git => {
            Source::Network(parsed)
        }
        gix::url::Scheme::Ext(ref s) => {
            return Err(Error::InvalidStore(format!(
                "{url}: unsupported scheme {s}"
            )));
        }
    })
}

fn refuse_ssh(url: &str) -> Error {
    Error::InvalidStore(format!(
        "{url}: an ssh transport needs an ssh process, and mdstore spawns none. \
         Declare the store with an https URL; the cache slot is the same for both forms."
    ))
}

fn open_isolated(dir: &Path) -> Result<gix::Repository> {
    gix::open_opts(dir, gix::open::Options::isolated())
        .map_err(|e| Error::InvalidStore(format!("{}: {e}", dir.display())))
}

/// gix errors are chains; the outer message alone hides the cause
/// ("An IO error occurred when talking to the server"), so the chain
/// is joined.
fn gix_err(context: &str, e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Error {
    let e: Box<dyn std::error::Error + Send + Sync> = e.into();
    let mut text = e.to_string();
    let mut cur = e.source();
    while let Some(inner) = cur {
        let msg = inner.to_string();
        if !text.ends_with(&msg) {
            text.push_str(": ");
            text.push_str(&msg);
        }
        cur = inner.source();
    }
    Error::InvalidStore(format!("{context}: {text}"))
}

/// True when a complete bare clone for this URL is present.
///
/// A slot left behind by an interrupted clone holds a HEAD and nothing
/// usable, so the check opens the slot as a repository.
pub fn is_cached(url: &str) -> bool {
    let dir = cache_dir(url);
    dir.join("HEAD").exists() && open_isolated(&dir).is_ok()
}

/// Make sure a bare clone for `url` is present, and return its directory.
///
/// The clone lands in a staging directory beside the slot and is renamed
/// into place, so a slot is either whole or absent. If two processes race
/// to fill one slot, the second finds it present and drops its own copy.
pub fn ensure_clone(url: &str) -> Result<PathBuf> {
    let dir = cache_dir(url);
    if dir.join("HEAD").exists() {
        ensure_slot_matches(&dir, url)?;
        sweep_staging(&dir);
        return Ok(dir);
    }
    let source = classify(url)?;
    if let Source::Ssh = source {
        return Err(refuse_ssh(url));
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A dead create's staging would otherwise sit forever: the sweep
    // on the present-slot path never runs while no slot exists. Only
    // a stale sibling goes — a live peer's staging is minutes old,
    // and an hour-old one belongs to nothing.
    sweep_stale_staging(&dir);
    let staging = dir.with_extension(format!("tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let result = match source {
        Source::Local(path) => mirror_local(&path, &staging, url, MirrorMode::Create).map(|_| ()),
        Source::Network(remote) => clone_network(remote, &staging),
        Source::Ssh => unreachable!("refused above"),
    };
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    if dir.join("HEAD").exists() {
        let _ = std::fs::remove_dir_all(&staging);
    } else {
        std::fs::rename(&staging, &dir)?;
    }
    Ok(dir)
}

/// A `<slot>.tmp-*` sibling older than an hour is an orphan from a
/// create that died, whether or not the slot exists.
///
/// The age is weaker than it looks: a write under `objects/` does not
/// move the staging root's mtime, so a create running longer than an
/// hour looks stale to a concurrent peer of the same slot. The damage
/// is bounded, because the rename is the only publish — the loser
/// fails its rename and a re-run fixes it. Age is chosen over a
/// pid-liveness check because pid liveness has no portable form.
///
/// The prefix test is what keeps this to this slot. Without it the
/// sweep takes every entry in the cache root older than an hour,
/// which is every other store a person has cached.
fn sweep_stale_staging(dir: &Path) {
    let (Some(parent), Some(name)) = (dir.parent(), dir.file_name().and_then(|n| n.to_str()))
    else {
        return;
    };
    let prefix = format!("{name}.tmp-");
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    let hour = std::time::Duration::from_secs(60 * 60);
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > hour);
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Once a slot is present, every `<slot>.tmp-*` sibling is an orphan
/// from an interrupted create: a peer that finds the slot present drops
/// its own staging anyway. Best effort; nothing depends on it.
fn sweep_staging(dir: &Path) {
    let (Some(parent), Some(name)) = (dir.parent(), dir.file_name().and_then(|n| n.to_str()))
    else {
        return;
    };
    let prefix = format!("{name}.tmp-");
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// The slot name merges https, scp, and `.git` spellings of one repository.
/// Two different repositories can still share a slot name only through a
/// hash collision, but a slot cloned from one URL must never serve
/// another, so the recorded origin is checked against the request.
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

const HEADS_MIRROR: &str = "+refs/heads/*:refs/heads/*";

fn clone_network(remote: gix::Url, staging: &Path) -> Result<()> {
    let mut prepare = gix::clone::PrepareFetch::new(
        remote,
        staging,
        gix::create::Kind::Bare,
        gix::create::Options::default(),
        gix::open::Options::isolated(),
    )
    .map_err(|e| gix_err("clone", e))?
    // A mirror layout, as `git clone --bare` makes: heads land in
    // refs/heads/*. gix adds its own remotes/origin refspec before this
    // closure runs, so the refspec is replaced, not appended.
    .configure_remote(|mut r| {
        r.replace_refspecs([HEADS_MIRROR], gix::remote::Direction::Fetch)?;
        Ok(r.with_fetch_tags(gix::remote::fetch::Tags::All))
    })
    .configure_connection(|conn| {
        conn.set_credentials(credential_fn());
        Ok(())
    });
    prepare
        .fetch_only(gix::progress::Discard, &AtomicBool::new(false))
        .map_err(|e| gix_err("clone", e))?;
    ensure_refs_dir(staging)
}

/// gix writes fetched refs packed and then removes the emptied `refs/`
/// tree, and a repository without `refs/` does not open. Put it back.
fn ensure_refs_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir.join("refs"))?;
    Ok(())
}

/// Bring the cache for `url` up to date: every head as the source has
/// it, heads the source dropped removed, HEAD as the source's HEAD.
pub fn fetch(url: &str) -> Result<()> {
    let dir = ensure_clone(url)?;
    let heads = match classify(url)? {
        Source::Ssh => return Err(refuse_ssh(url)),
        Source::Local(path) => mirror_local(&path, &dir, url, MirrorMode::Update)?,
        Source::Network(remote) => fetch_network(&dir, remote)?,
    };
    write_fetch_head(&dir, url, &heads)
}

/// The remote is built from the declared URL and the mirror refspec,
/// never read from the slot's config: a slot the git CLI made has no
/// fetch line, and a slot must never fetch from a URL that is not the
/// one declared.
fn fetch_network(dir: &Path, url: gix::Url) -> Result<Vec<(gix::ObjectId, String)>> {
    let mut repo = open_isolated(dir)?;
    repo.committer_or_set_generic_fallback()
        .map_err(|e| gix_err("fetch", e))?;
    let remote = repo
        .remote_at(url)
        .map_err(|e| gix_err("fetch", e))?
        .with_refspecs([HEADS_MIRROR], gix::remote::Direction::Fetch)
        .map_err(|e| gix_err("fetch", e))?
        .with_fetch_tags(gix::remote::fetch::Tags::All);
    let outcome = remote
        .connect(gix::remote::Direction::Fetch)
        .map_err(|e| gix_err("fetch", e))?
        .with_credentials(credential_fn())
        .prepare_fetch(gix::progress::Discard, fetch_ref_map_options()?)
        .map_err(|e| gix_err("fetch", e))?
        .receive(gix::progress::Discard, &AtomicBool::new(false))
        .map_err(|e| gix_err("fetch", e))?;

    // gix has no prune. A head the remote no longer maps to is deleted
    // by hand, which is what `git fetch --prune` did.
    let mut keep = std::collections::BTreeSet::new();
    let mut heads = Vec::new();
    for m in &outcome.ref_map.mappings {
        let Some(local) = m.local.as_ref() else {
            continue;
        };
        keep.insert(local.clone());
        if let Some(id) = m.remote.as_id() {
            heads.push((id.to_owned(), local.to_string()));
        }
    }
    // A head or tag the remote no longer has is deleted: the slot is a
    // mirror, so `git fetch --prune --prune-tags` is the model.
    let mut deletes = Vec::new();
    for prefix in ["refs/heads/", "refs/tags/"] {
        let refs = repo.references().map_err(|e| gix_err("fetch", e))?;
        for r in refs.prefixed(prefix).map_err(|e| gix_err("fetch", e))? {
            let r = r.map_err(|e| gix_err("fetch", e))?;
            if !keep.contains(r.name().as_bstr()) {
                deletes.push(delete_edit(r.name().to_owned()));
            }
        }
    }
    // HEAD follows the remote's HEAD when the remote says where it
    // points and that head was fetched. `git fetch` left HEAD alone; a
    // mirror whose upstream renamed its default branch would then read
    // a pruned ref forever.
    let remote_head = outcome.ref_map.remote_refs.iter().find_map(|r| match r {
        gix::protocol::handshake::Ref::Symbolic {
            full_ref_name,
            target,
            ..
        } if full_ref_name == "HEAD" => Some(target.clone()),
        _ => None,
    });
    if let Some(target) = remote_head
        && keep.contains(&target)
        && let Ok(name) = gix::refs::FullName::try_from(target)
    {
        let head: gix::refs::FullName = "HEAD".try_into().expect("HEAD is a valid ref name");
        repo.edit_reference(update_edit(head, gix::refs::Target::Symbolic(name)))
            .map_err(|e| gix_err("head", e))?;
    }
    repo.edit_references(deletes)
        .map_err(|e| gix_err("prune", e))?;
    ensure_refs_dir(dir)?;
    Ok(heads)
}

/// The ref map asks for HEAD as well as the heads and tags the refspec
/// covers. Over protocol v2 the server advertises only the requested
/// prefixes, and `refs/heads/` does not cover HEAD, so without this the
/// remote's HEAD is never seen and the slot's HEAD could not follow it.
fn fetch_ref_map_options() -> Result<gix::remote::ref_map::Options> {
    let head = gix::refspec::parse("HEAD".into(), gix::refspec::parse::Operation::Fetch)
        .map_err(|e| gix_err("refspec", e))?
        .to_owned();
    Ok(gix::remote::ref_map::Options {
        extra_refspecs: vec![head],
        ..Default::default()
    })
}

fn delete_edit(name: gix::refs::FullName) -> gix::refs::transaction::RefEdit {
    gix::refs::transaction::RefEdit {
        change: gix::refs::transaction::Change::Delete {
            expected: gix::refs::transaction::PreviousValue::Any,
            log: gix::refs::transaction::RefLog::AndReference,
        },
        name,
        deref: false,
    }
}

fn update_edit(
    name: gix::refs::FullName,
    target: gix::refs::Target,
) -> gix::refs::transaction::RefEdit {
    gix::refs::transaction::RefEdit {
        change: gix::refs::transaction::Change::Update {
            log: gix::refs::transaction::LogChange::default(),
            expected: gix::refs::transaction::PreviousValue::Any,
            new: target,
        },
        name,
        deref: false,
    }
}

/// gix writes no FETCH_HEAD. `seconds_since_fetch` reads its mtime, so
/// `fetch` writes it, in git's line format.
fn write_fetch_head(dir: &Path, url: &str, heads: &[(gix::ObjectId, String)]) -> Result<()> {
    let mut text = String::new();
    for (i, (id, name)) in heads.iter().enumerate() {
        let branch = name.strip_prefix("refs/heads/").unwrap_or(name);
        let mark = if i == 0 { "" } else { "not-for-merge" };
        text.push_str(&format!("{id}\t{mark}\tbranch '{branch}' of {url}\n"));
    }
    std::fs::write(dir.join("FETCH_HEAD"), text)?;
    Ok(())
}

#[derive(Clone, Copy)]
enum MirrorMode {
    Create,
    Update,
}

/// Fill or refresh a bare slot from a repository on this machine, by
/// reading its objects and refs directly. The result is what
/// `git clone --bare` then `git fetch --prune origin +refs/heads/*:refs/heads/*`
/// produced: every object reachable from the source's heads, the same
/// heads, the same HEAD, and `remote.origin.url` set to the declared URL.
///
/// Refs advance only after every object is copied, so an interrupted
/// copy leaves orphan objects and never a ref that points at a hole.
fn mirror_local(
    src_path: &Path,
    dst_dir: &Path,
    declared_url: &str,
    mode: MirrorMode,
) -> Result<Vec<(gix::ObjectId, String)>> {
    let src = gix::open_opts(src_path, gix::open::Options::isolated())
        .map_err(|e| gix_err(&format!("open {}", src_path.display()), e))?;
    if let MirrorMode::Create = mode {
        gix::init_bare(dst_dir).map_err(|e| gix_err("init", e))?;
    }
    // Reopened isolated: init_bare opens with the user's gitconfig, and
    // a url.insteadOf there could rewrite the saved URL.
    let mut dst = open_isolated(dst_dir)?;
    dst.committer_or_set_generic_fallback()
        .map_err(|e| gix_err("mirror", e))?;

    // 1. The source's heads and tags. A tag ref may point at a tag
    //    object; that object is copied too, and the walk starts from
    //    what it peels to. A detached source HEAD joins the walk, so a
    //    slot HEAD that mirrors it never points at a hole.
    let mut heads: Vec<(gix::refs::FullName, gix::ObjectId)> = Vec::new();
    let mut tags: Vec<(gix::refs::FullName, gix::ObjectId)> = Vec::new();
    let mut tips: Vec<gix::ObjectId> = Vec::new();
    let mut tag_objects: Vec<gix::ObjectId> = Vec::new();
    let mut tag_trees: Vec<gix::ObjectId> = Vec::new();
    let refs = src.references().map_err(|e| gix_err("mirror", e))?;
    for r in refs
        .prefixed("refs/heads/")
        .map_err(|e| gix_err("mirror", e))?
    {
        let mut r = r.map_err(|e| gix_err("mirror", e))?;
        let id = r.peel_to_id().map_err(|e| gix_err("mirror", e))?.detach();
        heads.push((r.name().to_owned(), id));
        tips.push(id);
    }
    let refs = src.references().map_err(|e| gix_err("mirror", e))?;
    for r in refs
        .prefixed("refs/tags/")
        .map_err(|e| gix_err("mirror", e))?
    {
        let mut r = r.map_err(|e| gix_err("mirror", e))?;
        let Some(direct) = r.target().try_id().map(gix::ObjectId::from) else {
            continue;
        };
        let peeled = r.peel_to_id().map_err(|e| gix_err("mirror", e))?.detach();
        if peeled != direct {
            tag_objects.push(direct);
        }
        tags.push((r.name().to_owned(), direct));
        // A tag may point at a tree or a blob. Those are copied as
        // objects; only a commit is a walk tip.
        match src.find_header(peeled).map(|h| h.kind()) {
            Ok(gix::objs::Kind::Commit) => tips.push(peeled),
            Ok(gix::objs::Kind::Tree) => tag_trees.push(peeled),
            Ok(_) => tag_objects.push(peeled),
            Err(e) => return Err(gix_err("mirror", e)),
        }
    }
    let detached_head = match src.head_name() {
        Ok(None) => src.head_id().ok().map(|id| id.detach()),
        _ => None,
    };
    tips.extend(detached_head);

    // 2. Every reachable object: commits, their trees, and every entry.
    let mut copier = Copier {
        src: &src,
        dst: &dst,
        buf: Vec::new(),
        seen: std::collections::HashSet::new(),
        pack: match mode {
            MirrorMode::Create => Some(Vec::new()),
            MirrorMode::Update => None,
        },
    };
    let walk = src.rev_walk(tips).all().map_err(|e| gix_err("mirror", e))?;
    for info in walk {
        let info = info.map_err(|e| gix_err("mirror", e))?;
        let commit = src.find_commit(info.id).map_err(|e| gix_err("mirror", e))?;
        let tree_id = commit.tree_id().map_err(|e| gix_err("mirror", e))?.detach();
        copier.tree(tree_id)?;
        copier.copy(info.id)?;
    }
    for id in tag_trees {
        copier.tree(id)?;
    }
    for id in tag_objects {
        copier.copy(id)?;
    }
    if let Some(ids) = copier.pack.take() {
        write_pack(&src, ids, &dst_dir.join("objects"))?;
    }

    // 3. Refs: every source head and tag upserted, every other dst head
    //    or tag deleted. The slot is a mirror.
    let names: std::collections::BTreeSet<_> =
        heads.iter().chain(&tags).map(|(n, _)| n.clone()).collect();
    let mut edits = Vec::new();
    for prefix in ["refs/heads/", "refs/tags/"] {
        let dst_refs = dst.references().map_err(|e| gix_err("mirror", e))?;
        for r in dst_refs
            .prefixed(prefix)
            .map_err(|e| gix_err("mirror", e))?
        {
            let r = r.map_err(|e| gix_err("mirror", e))?;
            if !names.contains(r.name()) {
                edits.push(delete_edit(r.name().to_owned()));
            }
        }
    }
    for (name, id) in heads.iter().chain(&tags) {
        edits.push(update_edit(name.clone(), gix::refs::Target::Object(*id)));
    }
    // HEAD mirrors the source: symbolic to the same branch when it has
    // one, else detached at the same commit, which the walk copied. An
    // unborn HEAD keeps the init default.
    let head_target = match src.head_name() {
        Ok(Some(branch)) => Some(gix::refs::Target::Symbolic(branch)),
        Ok(None) => detached_head.map(gix::refs::Target::Object),
        Err(_) => None,
    };
    if let Some(target) = head_target {
        let head: gix::refs::FullName = "HEAD".try_into().expect("HEAD is a valid ref name");
        edits.push(update_edit(head, target));
    }
    dst.edit_references(edits)
        .map_err(|e| gix_err("mirror", e))?;

    // 4. remote.origin.url, so ensure_slot_matches and store identity
    // keep working.
    if let MirrorMode::Create = mode {
        let mut cfg = gix::config::File::from_path_no_includes(
            dst_dir.join("config"),
            gix::config::Source::Local,
        )
        .map_err(|e| gix_err("config", e))?;
        let mut remote = dst
            .remote_at(declared_url)
            .map_err(|e| gix_err("config", e))?
            .with_refspecs([HEADS_MIRROR], gix::remote::Direction::Fetch)
            .map_err(|e| gix_err("config", e))?;
        remote
            .save_as_to("origin", &mut cfg)
            .map_err(|e| gix_err("config", e))?;
        std::fs::write(dst_dir.join("config"), cfg.to_bstring())?;
    }
    Ok(heads.iter().map(|(n, id)| (*id, n.to_string())).collect())
}

/// Copies objects from one repository's database to another's, once each.
/// How this crate indexes a pack it just wrote.
///
/// One constructor, used by write_pack and by the test that pins the
/// mode. Built inline in both places, the test pinned gix-pack's
/// behaviour and left this crate's choice of mode unguarded at the
/// only site that matters.
///
/// Verify re-reads every entry. Restore truncates at the first fault
/// and publishes what came before, which lands a slot whose pack
/// claims more objects than its index holds: every later read fails,
/// permanently, until a person deletes the slot.
fn bundle_options(object_hash: gix::hash::Kind) -> gix_pack::bundle::write::Options {
    gix_pack::bundle::write::Options {
        thread_limit: None,
        iteration_mode: gix_pack::data::input::Mode::Verify,
        index_version: gix_pack::index::Version::default(),
        object_hash,
        ..Default::default()
    }
}

/// One pack for a create's whole closure.
///
/// The ids arrive in walk order, commits before their trees and
/// blobs. The entries pipeline recompresses each object from the
/// source odb, the bytes writer emits a v2 pack, and the bundle
/// writer indexes that stream into objects/pack. The source is read
/// through its own thread-safe handle because the entries pipeline
/// wants Send, and a Repository's handle is not.
fn write_pack(src: &gix::Repository, ids: Vec<gix::ObjectId>, dst_objects: &Path) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let count = u32::try_from(ids.len())
        .map_err(|_| Error::InvalidStore("more than u32::MAX objects in one mirror".into()))?;
    // common_dir, not git_dir: a linked worktree's git_dir holds no
    // objects directory, and the objects live in the common dir. The
    // two are the same path for an ordinary repository. The loose arm
    // never had this, because src.objects already follows it.
    let find = gix_odb::at(src.common_dir().join("objects"))
        .and_then(gix_odb::Handle::into_arc)
        .map_err(|e| Error::InvalidStore(format!("open source objects: {e}")))?;
    let counts: Vec<gix_pack::data::output::Count> = ids
        .into_iter()
        .map(|id| gix_pack::data::output::Count::from_data(id, None))
        .collect();
    let chunks = gix_pack::data::output::entry::iter_from_counts(
        counts,
        find,
        Box::new(gix::progress::Discard),
        gix_pack::data::output::entry::iter_from_counts::Options {
            version: gix_pack::data::Version::V2,
            allow_thin_pack: false,
            ..Default::default()
        },
    );
    let entries = gix::features::parallel::InOrderIter::from(chunks);
    let mut pack = Vec::new();
    let mut writer = gix_pack::data::output::bytes::FromEntriesIter::new(
        entries,
        &mut pack,
        count,
        gix_pack::data::Version::V2,
        src.object_hash(),
    );
    for step in &mut writer {
        step.map_err(|e| Error::InvalidStore(format!("write pack: {e}")))?;
    }
    drop(writer);
    gix_pack::Bundle::write_to_directory(
        &mut pack.as_slice(),
        Some(&dst_objects.join("pack")),
        &mut gix::progress::Discard,
        &std::sync::atomic::AtomicBool::new(false),
        None::<gix::objs::find::Never>,
        bundle_options(src.object_hash()),
    )
    .map_err(|e| Error::InvalidStore(format!("index pack: {e}")))?;
    Ok(())
}

struct Copier<'a> {
    src: &'a gix::Repository,
    dst: &'a gix::Repository,
    buf: Vec<u8>,
    seen: std::collections::HashSet<gix::ObjectId>,
    /// On a create, ids collect here and one pack is written at the
    /// end. Written loose, a 68 MB source became 10,395 files and a
    /// 12 second first sync, and the slot only ever grew. An update
    /// keeps the loose write for its small delta; None means loose.
    pack: Option<Vec<gix::ObjectId>>,
}

impl Copier<'_> {
    fn copy(&mut self, id: gix::ObjectId) -> Result<()> {
        use gix::objs::Exists as _;
        use gix::prelude::{Find as _, Write as _};
        if !self.seen.insert(id) {
            return Ok(());
        }
        if self.dst.objects.exists(&id) {
            return Ok(());
        }
        if let Some(ids) = &mut self.pack {
            // Existence in the source is still checked per object, so
            // a hole in the source fails the mirror here, not at pack
            // time with a less specific message.
            if !self.src.objects.exists(&id) {
                return Err(Error::InvalidStore(format!(
                    "object {id} is missing from the source"
                )));
            }
            ids.push(id);
            return Ok(());
        }
        let data = self
            .src
            .objects
            .try_find(&id, &mut self.buf)
            .map_err(|e| gix_err("read object", e))?
            .ok_or_else(|| {
                Error::InvalidStore(format!("object {id} is missing from the source"))
            })?;
        self.dst
            .objects
            .write_buf(data.kind, data.data)
            .map_err(|e| gix_err("write object", e))?;
        Ok(())
    }

    /// A tree and everything under it: subtrees recursively, blobs as
    /// leaves. Submodule commits are skipped; they live elsewhere.
    fn tree(&mut self, id: gix::ObjectId) -> Result<()> {
        if self.seen.contains(&id) {
            return Ok(());
        }
        // Subtrees and blobs are written before their tree, so a tree
        // the destination holds implies its whole closure is there.
        {
            use gix::objs::Exists as _;
            if self.dst.objects.exists(&id) {
                self.seen.insert(id);
                return Ok(());
            }
        }
        let tree = self
            .src
            .find_tree(id)
            .map_err(|e| gix_err("read tree", e))?;
        let mut subtrees = Vec::new();
        let mut blobs = Vec::new();
        for entry in tree.iter() {
            let entry = entry.map_err(|e| gix_err("read tree", e))?;
            let mode = entry.mode();
            if mode.is_tree() {
                subtrees.push(entry.oid().to_owned());
            } else if mode.is_blob() || mode.is_link() {
                blobs.push(entry.oid().to_owned());
            }
        }
        for b in blobs {
            self.copy(b)?;
        }
        for t in subtrees {
            self.tree(t)?;
        }
        self.copy(id)
    }
}

/// Resolve a revision to a commit id in a cache slot. An unknown name is
/// an error, never an echo, so a bad pin cannot read as an empty store.
pub fn resolve_rev(dir: &Path, rev: Option<&str>) -> Result<String> {
    let repo = open_isolated(dir)?;
    let spec = match rev {
        Some(r) => format!("{r}^{{commit}}"),
        None => "HEAD^{commit}".to_string(),
    };
    let id = repo
        .rev_parse_single(spec.as_str())
        .map_err(|e| gix_err(&format!("rev-parse {spec}"), e))?;
    Ok(id.to_hex().to_string())
}

/// The files under `prefix` at `rev`, relative to the prefix, as
/// `git ls-tree -r --name-only rev:prefix` listed them. A prefix that
/// does not exist at that revision is an empty store, not an error.
/// Only blobs are listed; symlinks and submodules are not documents.
pub fn list_tree(dir: &Path, rev: &str, prefix: &str) -> Result<Vec<String>> {
    let Ok(repo) = open_isolated(dir) else {
        return Ok(Vec::new());
    };
    let spec = if prefix.is_empty() {
        rev.to_string()
    } else {
        format!("{rev}:{prefix}")
    };
    let Ok(id) = repo.rev_parse_single(spec.as_str()) else {
        return Ok(Vec::new());
    };
    let Ok(obj) = id.object() else {
        return Ok(Vec::new());
    };
    let Ok(tree) = obj.peel_to_tree() else {
        return Ok(Vec::new());
    };
    let entries = tree
        .traverse()
        .breadthfirst
        .files()
        .map_err(|e| gix_err("ls-tree", e))?;
    Ok(entries
        .into_iter()
        .filter(|e| e.mode.is_blob())
        .map(|e| e.filepath.to_string())
        .collect())
}

/// One file's text at `rev:path`.
pub fn show(dir: &Path, rev: &str, path: &str) -> Result<String> {
    let repo = open_isolated(dir)?;
    let spec = format!("{rev}:{path}");
    let obj = repo
        .rev_parse_single(spec.as_str())
        .map_err(|e| gix_err(&spec, e))?
        .object()
        .map_err(|e| gix_err(&spec, e))?;
    let blob = obj
        .try_into_blob()
        .map_err(|_| Error::InvalidStore(format!("{spec}: not a file")))?;
    Ok(String::from_utf8_lossy(&blob.data).into_owned())
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

/// The origin remote of a checkout, for store identity. The raw config
/// value is read, so no `url.insteadOf` rewrite is applied.
pub fn origin_url(path: &Path) -> Option<String> {
    if !path.join(".git").exists() && !path.join("HEAD").exists() {
        return None;
    }
    let repo = open_isolated(path).ok()?;
    let raw = repo.config_snapshot().string("remote.origin.url")?;
    let s = raw.to_string();
    (!s.is_empty()).then_some(s)
}

/// The credential source for https, in-process: userinfo in the URL,
/// then git-credential-store's file. No helper program runs and nothing
/// prompts. gix asks only after a 401, so a public store never gets here.
/// Public so a consumer that fetches with gix itself uses the same one.
// The error type is gix's; its size is not this crate's to shrink.
#[allow(clippy::result_large_err)]
pub fn credential_fn()
-> impl FnMut(gix::credentials::helper::Action) -> gix::credentials::protocol::Result {
    |action| {
        let Some(ctx) = action.context() else {
            return Ok(None);
        };
        let Some(url) = ctx.url.as_ref() else {
            return Ok(None);
        };
        let Ok(parsed) = gix::url::parse(url.as_bstr()) else {
            return Ok(None);
        };
        let found = match (parsed.user(), parsed.password()) {
            (Some(u), Some(p)) => Some((u.to_owned(), p.to_owned())),
            _ => credentials_from_store_file(&parsed),
        };
        let Some((username, password)) = found else {
            return Ok(None);
        };
        Ok(Some(gix::credentials::protocol::Outcome {
            identity: gix::sec::identity::Account {
                username,
                password,
                oauth_refresh_token: None,
            },
            next: ctx.clone().into(),
        }))
    }
}

/// `~/.git-credentials` and `$XDG_CONFIG_HOME/git/credentials`, one
/// `scheme://user:pass@host[/path]` per line. The first line whose
/// scheme and host match wins, as git-credential-store(1) resolves it.
fn credentials_from_store_file(want: &gix::Url) -> Option<(String, String)> {
    let mut files = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        files.push(PathBuf::from(&home).join(".git-credentials"));
        files.push(PathBuf::from(&home).join(".config/git/credentials"));
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        files.push(PathBuf::from(xdg).join("git/credentials"));
    }
    let want_host = want.host()?;
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Ok(entry) = gix::url::parse(gix::bstr::BStr::new(line)) else {
                continue;
            };
            if entry.scheme != want.scheme || entry.host() != Some(want_host) {
                continue;
            }
            // git-credential-store matches the username too, when the
            // URL carries one.
            if let Some(u) = want.user()
                && entry.user() != Some(u)
            {
                continue;
            }
            if let (Some(u), Some(p)) = (entry.user(), entry.password()) {
                return Some((u.to_owned(), p.to_owned()));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "mdstore-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn sig() -> gix::actor::Signature {
        gix::actor::Signature {
            name: "t".into(),
            email: "t@e".into(),
            time: gix::date::Time::new(1_600_000_000, 0),
        }
    }

    /// Write `files` (path → text) as a tree, nested by `/`, and commit
    /// it on HEAD. Returns the commit id. All through gix; no git process.
    fn commit_files(
        repo: &gix::Repository,
        files: &[(&str, &str)],
        message: &str,
    ) -> gix::ObjectId {
        use std::collections::BTreeMap;
        #[derive(Default)]
        struct Dir {
            files: BTreeMap<String, gix::ObjectId>,
            dirs: BTreeMap<String, Dir>,
        }
        let mut root = Dir::default();
        for (path, text) in files {
            let oid = repo.write_blob(text.as_bytes()).unwrap().detach();
            let mut parts: Vec<&str> = path.split('/').collect();
            let name = parts.pop().unwrap().to_string();
            let mut node = &mut root;
            for part in parts {
                node = node.dirs.entry(part.to_string()).or_default();
            }
            node.files.insert(name, oid);
        }
        fn write(repo: &gix::Repository, d: &Dir) -> gix::ObjectId {
            let mut entries = Vec::new();
            for (name, oid) in &d.files {
                entries.push(gix::objs::tree::Entry {
                    mode: gix::objs::tree::EntryKind::Blob.into(),
                    filename: name.as_str().into(),
                    oid: *oid,
                });
            }
            for (name, sub) in &d.dirs {
                entries.push(gix::objs::tree::Entry {
                    mode: gix::objs::tree::EntryKind::Tree.into(),
                    filename: name.as_str().into(),
                    oid: write(repo, sub),
                });
            }
            let mut tree = gix::objs::Tree { entries };
            tree.entries.sort();
            repo.write_object(&tree).unwrap().detach()
        }
        let tree = write(repo, &root);
        let parents: Vec<gix::ObjectId> = repo
            .head_id()
            .ok()
            .map(|id| id.detach())
            .into_iter()
            .collect();
        let s = sig();
        repo.commit_as(
            s.to_ref(&mut gix::date::parse::TimeBuf::default()),
            s.to_ref(&mut gix::date::parse::TimeBuf::default()),
            "HEAD",
            message,
            tree,
            parents,
        )
        .unwrap()
        .detach()
    }

    fn init(dir: &Path) -> gix::Repository {
        // The fixture edits references, and a reflog write needs a
        // committer. The identity lives in the repo's own config, so
        // the tests do not depend on the machine's global gitconfig.
        // A host with no global config (a CI runner) fails without it.
        let repo = gix::init(dir).unwrap();
        let config = repo.git_dir().join("config");
        let mut text = std::fs::read_to_string(&config).unwrap();
        text.push_str("[user]\n\tname = t\n\temail = t@e\n");
        std::fs::write(&config, text).unwrap();
        gix::open(dir).unwrap()
    }

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
        let base = scratch("slot");
        init(&base);
        std::fs::OpenOptions::new()
            .append(true)
            .open(base.join(".git/config"))
            .unwrap()
            .write_all(b"[remote \"origin\"]\n\turl = https://example.com/org/other\n")
            .unwrap();
        assert_eq!(
            origin_url(&base).as_deref(),
            Some("https://example.com/org/other")
        );

        let err = ensure_slot_matches(&base, "https://example.com/org/kb")
            .unwrap_err()
            .to_string();
        assert!(err.contains("already holds"), "{err}");

        // The same repository written another way is still a match.
        assert!(ensure_slot_matches(&base, "git@example.com:org/other.git").is_ok());
    }

    use std::io::Write as _;

    #[test]
    fn a_revision_that_does_not_exist_is_an_error() {
        // A rev-parse without ^{commit} would echo an unknown name back,
        // and the pin then reads as an empty store instead of failing.
        let base = scratch("rev");
        let repo = init(&base);
        commit_files(&repo, &[("a.md", "---\ntitle: a\n---\n")], "one");
        assert!(resolve_rev(&base, None).is_ok());
        assert!(
            resolve_rev(&base, Some("no-such-tag")).is_err(),
            "an unknown revision must fail"
        );
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
    fn an_ssh_url_is_refused_with_the_fix_in_the_message() {
        let _env = crate::env_lock();
        let base = scratch("ssh");
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };
        let err = ensure_clone("git@example.com:org/kb.git")
            .unwrap_err()
            .to_string();
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
        assert!(err.contains("https"), "{err}");
        assert!(
            !base.join("cache").exists()
                || std::fs::read_dir(base.join("cache"))
                    .unwrap()
                    .next()
                    .is_none()
        );
    }

    /// A first sync writes a pack, not a loose file per object.
    ///
    /// The mirror wrote every copied object loose. A 68 MB source
    /// produced 10,395 loose files and a 12 second first sync, and
    /// the slot only ever grew. The create writes one pack now; an
    /// update still writes its small delta loose.
    #[test]
    fn a_create_packs_its_objects_and_they_all_read_back() {
        let _env = crate::env_lock();
        let base = scratch("packed");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let repo = init(&origin);
        // Enough files that loose-vs-packed is unambiguous.
        let files: Vec<(String, String)> = (0..40)
            .map(|i| {
                (
                    format!("notes/n{i:02}.md"),
                    format!("---\ntitle: N{i}\n---\nbody {i}\n"),
                )
            })
            .collect();
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let c1 = commit_files(&repo, &refs, "one");
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };

        let url = origin.display().to_string();
        let dir = ensure_clone(&url).unwrap();

        let loose = count_loose(&dir);
        let packs = std::fs::read_dir(dir.join("objects/pack"))
            .map(|it| {
                it.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "pack"))
                    .count()
            })
            .unwrap_or(0);
        assert!(packs >= 1, "the create wrote no pack");
        assert!(
            loose < 5,
            "the create wrote its objects loose: {loose} loose files"
        );

        // Every object reads back through the pack.
        let rev = resolve_rev(&dir, None).unwrap();
        assert_eq!(rev, c1.to_string());
        for (path, body) in &files {
            assert_eq!(&show(&dir, &rev, path).unwrap(), body, "{path} lost");
        }

        // An update after the create still lands, loose or not.
        commit_files(
            &repo,
            &[("notes/extra.md", "---\ntitle: Extra\n---\n")],
            "two",
        );
        fetch(&url).unwrap();
        let r2 = resolve_rev(&dir, None).unwrap();
        assert!(
            show(&dir, &r2, "notes/extra.md").unwrap().contains("Extra"),
            "the update's object is unreadable"
        );
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    fn count_loose(dir: &Path) -> usize {
        let mut n = 0;
        if let Ok(entries) = std::fs::read_dir(dir.join("objects")) {
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if name.len() == 2 && name.chars().all(|c| c.is_ascii_hexdigit()) {
                    n += std::fs::read_dir(e.path())
                        .map(|it| it.count())
                        .unwrap_or(0);
                }
            }
        }
        n
    }

    /// An orphan staging directory from an interrupted create is
    /// swept before the next create.
    ///
    /// The sweep ran only when the slot already existed. A create that
    /// never completed left <slot>.tmp-<pid> forever, because the next
    /// run took the create path, which removed only its own pid's
    /// staging. A stale orphan is one older than an hour: a live
    /// peer's staging is minutes old, and deleting it would only fail
    /// that peer's rename, not corrupt anything.
    #[test]
    fn an_orphan_from_a_dead_create_is_swept_on_the_next_create() {
        let _env = crate::env_lock();
        let base = scratch("orphan-create");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let repo = init(&origin);
        commit_files(&repo, &[("notes/a.md", "---\ntitle: A\n---\n")], "one");
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };

        let url = origin.display().to_string();
        let dir = cache_dir(&url);
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();

        // An old orphan and a fresh one, planted before any slot
        // exists, so the create path is the one that must sweep.
        let stale = dir.with_extension("tmp-424242");
        std::fs::create_dir_all(&stale).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60);
        let times = std::fs::FileTimes::new()
            .set_accessed(old)
            .set_modified(old);
        std::fs::File::open(&stale)
            .unwrap()
            .set_times(times)
            .unwrap();
        let fresh = dir.with_extension("tmp-424243");
        std::fs::create_dir_all(&fresh).unwrap();

        // Another store's slot, and its own stale staging, both aged
        // past the cutoff so only the prefix test can spare them.
        let neighbour = dir.parent().unwrap().join("some-other-slot");
        std::fs::create_dir_all(&neighbour).unwrap();
        std::fs::write(neighbour.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let neighbour_staging = neighbour.with_extension("tmp-424244");
        std::fs::create_dir_all(&neighbour_staging).unwrap();
        for p in [&neighbour, &neighbour_staging] {
            std::fs::File::open(p).unwrap().set_times(times).unwrap();
        }

        ensure_clone(&url).unwrap();
        assert!(!stale.exists(), "a stale orphan survived the create path");
        assert!(
            fresh.exists(),
            "a fresh staging directory was swept while its create could be live"
        );
        assert!(dir.join("HEAD").exists(), "the clone itself failed");

        // A neighbouring slot is not this slot's staging. Without the
        // prefix test the sweep takes every entry in the cache root
        // older than an hour, which is every other store a person has
        // cached.
        assert!(
            neighbour.join("HEAD").exists(),
            "the sweep deleted another store's slot"
        );
        assert!(
            neighbour_staging.exists(),
            "the sweep deleted another slot's staging"
        );
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    /// A corrupt pack never becomes a slot.
    ///
    /// The bundle writer re-parses the stream, and iteration_mode
    /// Verify is what makes a bad byte an error. Under Restore the
    /// create succeeded and published a slot whose pack claimed more
    /// objects than its index held: every later read failed, forever,
    /// until a person deleted the slot by hand.
    #[test]
    fn a_corrupt_pack_is_refused_rather_than_published() {
        let _env = crate::env_lock();
        let base = scratch("corruptpack");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let repo = init(&origin);
        commit_files(
            &repo,
            &[("notes/a.md", "---\ntitle: A\n---\nbody\n")],
            "one",
        );
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };

        let url = origin.display().to_string();
        let dir = cache_dir(&url);
        // A real pack, then one byte flipped inside it. Junk bytes
        // are refused by any mode, so they pin nothing; only a
        // structurally valid pack with corrupt content separates
        // Verify from Restore, which truncates and accepts.
        ensure_clone(&url).expect("a sound source failed to mirror");
        let packs: Vec<std::path::PathBuf> = std::fs::read_dir(dir.join("objects/pack"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pack"))
            .collect();
        assert_eq!(packs.len(), 1, "expected exactly one pack to corrupt");
        let mut bytes = std::fs::read(&packs[0]).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xff;

        let out = base.join("packout");
        std::fs::create_dir_all(&out).unwrap();
        let refused = gix_pack::Bundle::write_to_directory(
            &mut bytes.as_slice(),
            Some(&out),
            &mut gix::progress::Discard,
            &std::sync::atomic::AtomicBool::new(false),
            None::<gix::objs::find::Never>,
            bundle_options(gix::hash::Kind::Sha1),
        );
        assert!(
            refused.is_err(),
            "Verify accepted a pack that is not one; a corrupt stream would reach a slot"
        );

        // The sound pack indexes through the same call, so the
        // assertion above is about the corruption and not about the
        // call failing always.
        let sound = std::fs::read(&packs[0]).unwrap();
        let out2 = base.join("packout2");
        std::fs::create_dir_all(&out2).unwrap();
        gix_pack::Bundle::write_to_directory(
            &mut sound.as_slice(),
            Some(&out2),
            &mut gix::progress::Discard,
            &std::sync::atomic::AtomicBool::new(false),
            None::<gix::objs::find::Never>,
            bundle_options(gix::hash::Kind::Sha1),
        )
        .expect("a sound pack was refused");
        assert!(dir.join("HEAD").exists());
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    /// A source missing an object fails the mirror by name.
    ///
    /// The collect arm checks existence per object so a hole is named
    /// here. Without the check the pack pipeline still fails, but as
    /// "a pack entry could not be extracted", which names neither the
    /// object nor the source.
    #[test]
    fn a_hole_in_the_source_names_the_object() {
        let _env = crate::env_lock();
        let base = scratch("srchole");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let repo = init(&origin);
        commit_files(&repo, &[("notes/a.md", "---\ntitle: A\n---\n")], "one");
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };

        // Delete the BLOB, not a tree: the walk reads a tree itself
        // and would fail there first, so only a missing leaf reaches
        // the collect arm's check.
        let src = gix::open_opts(&origin, gix::open::Options::isolated()).unwrap();
        let blob = src
            .rev_parse_single("HEAD:notes/a.md")
            .unwrap()
            .detach()
            .to_string();
        let removed = blob.clone();
        std::fs::remove_file(
            origin
                .join(".git/objects")
                .join(&blob[..2])
                .join(&blob[2..]),
        )
        .unwrap();
        drop(src);

        let url = origin.display().to_string();
        let err = ensure_clone(&url).expect_err("a source with a hole mirrored anyway");
        let msg = err.to_string();
        assert!(
            msg.contains("missing from the source"),
            "the hole was not named as a source problem: {msg}"
        );
        assert!(
            msg.contains(&removed[..8]),
            "the message does not name the object: {msg}"
        );
        assert!(
            !crate::git::cache_dir(&url).join("HEAD").exists(),
            "a slot was published from an incomplete source"
        );
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    /// A linked worktree is a legitimate source.
    ///
    /// write_pack opened the source odb at git_dir()/objects. For a
    /// linked worktree git_dir() is <main>/.git/worktrees/<name>,
    /// which holds no objects directory; the objects live in the
    /// common dir. The mirror failed outright and made no slot. The
    /// loose path never had this, because it goes through
    /// src.objects, which follows the common dir.
    #[test]
    fn a_linked_worktree_source_mirrors() {
        let _env = crate::env_lock();
        let base = scratch("worktree-src");
        let main = base.join("main");
        std::fs::create_dir_all(&main).unwrap();
        let repo = init(&main);
        let c1 = commit_files(&repo, &[("notes/a.md", "---\ntitle: A\n---\n")], "one");
        // The linked-worktree layout, written directly: this crate
        // spawns no program, and its own integration test enforces
        // that. A worktree is a .git FILE naming an administrative
        // directory under the main repository, and that directory
        // says where the common dir is.
        let linked = base.join("linked");
        std::fs::create_dir_all(&linked).unwrap();
        let admin = main.join(".git/worktrees/linked");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", linked.join(".git").display()),
        )
        .unwrap();
        std::fs::write(admin.join("HEAD"), format!("{c1}\n")).unwrap();
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };

        // The fixture is only a fixture if it really is a worktree:
        // the two dirs must differ, and the git dir must be the one
        // without objects. Otherwise this passes for the wrong reason.
        let probe = gix::open_opts(&linked, gix::open::Options::isolated()).unwrap();
        assert_ne!(
            probe.git_dir(),
            probe.common_dir(),
            "the fixture is not a linked worktree"
        );
        assert!(
            !probe.git_dir().join("objects").is_dir(),
            "the fixture's git dir has objects, so it cannot catch the bug"
        );
        drop(probe);

        let url = linked.display().to_string();
        let dir = ensure_clone(&url).expect("a linked worktree source failed to mirror");
        let rev = resolve_rev(&dir, None).unwrap();
        assert_eq!(rev, c1.to_string());
        assert!(
            show(&dir, &rev, "notes/a.md").unwrap().contains("A"),
            "the worktree's content is missing from the slot"
        );
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    /// The pack carries shapes the walk can produce beyond a plain
    /// commit chain.
    ///
    /// The entries pipeline gets ids the walk collected, so anything
    /// the walk can reach has to survive the round trip: one blob
    /// reachable by many paths (counted once, by the seen set), an
    /// empty tree, a tag pointing at a tree, and a nested tag.
    #[test]
    fn a_packed_create_carries_every_shape_the_walk_reaches() {
        let _env = crate::env_lock();
        let base = scratch("packshapes");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let repo = init(&origin);
        // The same content under four paths: one blob, four entries.
        let same = "---\ntitle: Same\n---\nidentical\n";
        let c1 = commit_files(
            &repo,
            &[
                ("notes/a.md", same),
                ("notes/b.md", same),
                ("deep/one/c.md", same),
                ("deep/two/d.md", same),
            ],
            "one",
        );
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };

        let url = origin.display().to_string();
        let dir = ensure_clone(&url).unwrap();
        let rev = resolve_rev(&dir, None).unwrap();
        assert_eq!(rev, c1.to_string());
        for path in ["notes/a.md", "notes/b.md", "deep/one/c.md", "deep/two/d.md"] {
            assert_eq!(show(&dir, &rev, path).unwrap(), same, "{path} lost");
        }
        // Every path is listed, so no tree entry went missing.
        let listed = list_tree(&dir, &rev, "deep").unwrap();
        assert_eq!(listed.len(), 2, "a subtree lost entries: {listed:?}");

        // The slot verifies as a whole: the pack's index and every
        // object it claims.
        let slot = gix::open_opts(&dir, gix::open::Options::isolated()).unwrap();
        for path in ["notes/a.md", "deep/one/c.md"] {
            let id = slot
                .rev_parse_single(format!("{rev}:{path}").as_str())
                .expect("an object the pack should hold is unreachable");
            assert!(slot.find_object(id).is_ok(), "{path} is indexed but absent");
        }
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    /// A file:// declaration is already absolute and stays untouched.
    ///
    /// The rewrite path-joined it, because is_remote_url is
    /// deliberately false for file://; the mangled text then resolved
    /// to nothing, and the member degraded to a permanent
    /// 'not in the cache' no sync could satisfy. Root-level file://
    /// is exactly the legitimate case: the root may name anything on
    /// its own machine.
    #[test]
    fn a_file_scheme_declaration_is_not_path_joined() {
        let _env = crate::env_lock();
        let base = scratch("filescheme");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let repo = init(&origin);
        commit_files(&repo, &[("notes/a.md", "---\ntitle: A\n---\n")], "one");
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("stores.yml"),
            format!(
                "stores:\n  - alias: up\n    git: file://{}\n",
                origin.display()
            ),
        )
        .unwrap();
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };

        let graph = crate::store::StoreGraph::open(&root, &crate::store::LocalPaths).unwrap();
        let url = graph
            .members
            .iter()
            .find_map(|m| match &m.source {
                crate::store::StoreSource::Git { url, .. } => Some(url.clone()),
                _ => None,
            })
            .expect("the declared git member is missing");
        assert!(
            url.starts_with("file://"),
            "the file scheme was path-joined away: {url}"
        );
        assert!(
            !url.contains(&root.display().to_string()),
            "the declaration was joined onto the root: {url}"
        );
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    /// A relative git declaration names a repository next to the
    /// store that declared it, not next to the process.
    ///
    /// The declared text reached the cache untouched, so `git: ../up`
    /// in two different roots keyed one slot, the second root read the
    /// first root's mirror, and the fetch itself resolved against the
    /// process cwd.
    #[test]
    fn a_relative_git_declaration_resolves_against_its_store() {
        let _env = crate::env_lock();
        let base = scratch("reldecl");
        for (side, body) in [("A", "alpha"), ("B", "beta")] {
            let up = base.join(side).join("up");
            std::fs::create_dir_all(&up).unwrap();
            let repo = init(&up);
            commit_files(
                &repo,
                &[("notes/which.md", &format!("---\ntitle: {body}\n---\n"))],
                "one",
            );
            let root = base.join(side).join("root");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(
                root.join("stores.yml"),
                "stores:\n  - alias: up\n    git: ../up\n",
            )
            .unwrap();
        }
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };

        let mut slots = Vec::new();
        for side in ["A", "B"] {
            let root = base.join(side).join("root");
            let graph = crate::store::StoreGraph::open(&root, &crate::store::LocalPaths).unwrap();
            let up = graph
                .members
                .iter()
                .find_map(|m| match &m.source {
                    crate::store::StoreSource::Git { url, .. } => Some(url.clone()),
                    _ => None,
                })
                .expect("the declared git member is missing");
            assert!(
                std::path::Path::new(&up).is_absolute(),
                "the declaration left the walk still relative: {up}"
            );
            crate::store::sync_source(&crate::store::StoreSource::Git {
                url: up.clone(),
                rev: None,
            })
            .expect("sync of the resolved declaration failed");
            let slot = crate::git::cache_dir(&up);
            let rev = crate::git::resolve_rev(&slot, None).unwrap();
            let body = crate::git::show(&slot, &rev, "notes/which.md").unwrap();
            let want = if side == "A" { "alpha" } else { "beta" };
            assert!(
                body.contains(want),
                "{side}'s member read the other root's mirror: {body}"
            );
            slots.push(slot);
        }
        assert_ne!(
            slots[0], slots[1],
            "two different repositories share one slot"
        );
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    /// sync answers for the pin, not only for the fetch.
    ///
    /// fetch never read the declared rev, so a bad pin reported
    /// synced and surfaced only on read, as a gix-internal message.
    #[test]
    fn a_sync_of_a_pin_the_source_does_not_hold_says_so() {
        let _env = crate::env_lock();
        let base = scratch("badpin");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let repo = init(&origin);
        commit_files(&repo, &[("notes/a1.md", "---\ntitle: A\n---\n")], "one");
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };

        let url = origin.display().to_string();
        let source = crate::store::StoreSource::Git {
            url: url.clone(),
            rev: Some("badbadbadbadbadbadbadbadbadbadbadbadbad0".into()),
        };
        let err = crate::store::sync_source(&source)
            .expect_err("a pin the source does not hold reported synced");
        let msg = err.to_string();
        assert!(msg.contains("pin"), "the message does not say pin: {msg}");
        assert!(
            msg.contains("badbadbad"),
            "the message does not name the pin: {msg}"
        );
        assert!(
            msg.contains(&url),
            "the message does not name the source: {msg}"
        );

        // The good pin, through the same path, still syncs.
        let good = crate::git::resolve_rev(&crate::git::cache_dir(&url), None).unwrap();
        let source = crate::store::StoreSource::Git {
            url,
            rev: Some(good),
        };
        crate::store::sync_source(&source).expect("a held pin failed to sync");
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    /// A source with no commits is said plainly.
    ///
    /// An unborn HEAD surfaced as a gix delegate.peel_until message,
    /// which names the library and not the problem.
    #[test]
    fn a_sync_of_a_source_with_no_commits_names_that() {
        let _env = crate::env_lock();
        let base = scratch("unborn");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let _repo = init(&origin);
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };

        let url = origin.display().to_string();
        let source = crate::store::StoreSource::Git { url, rev: None };
        let err = crate::store::sync_source(&source)
            .expect_err("a source with no commits reported synced");
        let msg = err.to_string();
        assert!(
            msg.contains("no commits"),
            "the message does not say no commits: {msg}"
        );
        assert!(
            !msg.contains("peel"),
            "a gix internal leaked to the user: {msg}"
        );
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    #[test]
    fn a_real_repository_round_trips_through_the_cache() {
        // Build a repository, mirror it into the cache, read a file from
        // objects at a pinned revision, then fetch a later commit and a
        // dropped branch.
        let _env = crate::env_lock();
        let base = scratch("git");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let repo = init(&origin);
        let first = commit_files(
            &repo,
            &[("notes/a1-first.md", "---\ntitle: First\n---\n")],
            "one",
        );
        // A second commit that the pinned revision must not see.
        commit_files(
            &repo,
            &[
                ("notes/a1-first.md", "---\ntitle: First\n---\n"),
                ("notes/b2-second.md", "---\ntitle: Second\n---\n"),
            ],
            "two",
        );
        // A side branch that the fetch must prune once it is gone.
        let side: gix::refs::FullName = "refs/heads/side".try_into().unwrap();
        repo.edit_reference(update_edit(side.clone(), gix::refs::Target::Object(first)))
            .unwrap();

        let url = format!("file://{}", origin.display());
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };
        let dir = ensure_clone(&url).unwrap();
        assert!(is_cached(&url));
        assert_eq!(
            origin_url(&dir).as_deref(),
            Some(url.as_str()),
            "the slot records the declared URL"
        );

        let head = resolve_rev(&dir, None).unwrap();
        assert_eq!(list_tree(&dir, &head, "notes").unwrap().len(), 2);
        assert!(
            resolve_rev(&dir, Some("side")).is_ok(),
            "the side branch was mirrored"
        );

        // The pin sees only the first commit, and both revisions read
        // out of the same slot.
        assert_eq!(
            list_tree(&dir, &first.to_string(), "notes").unwrap(),
            vec!["a1-first.md"]
        );
        let text = show(&dir, &first.to_string(), "notes/a1-first.md").unwrap();
        assert!(text.contains("title: First"));
        assert!(
            list_tree(&dir, &head, "no-such-dir").unwrap().is_empty(),
            "a missing prefix is an empty store"
        );

        // Move the source on, drop the side branch, fetch: the slot
        // follows, and the pruned branch is gone.
        commit_files(
            &repo,
            &[("notes/c3-third.md", "---\ntitle: Third\n---\n")],
            "three",
        );
        repo.edit_reference(delete_edit(side)).unwrap();
        fetch(&url).unwrap();
        let head2 = resolve_rev(&dir, None).unwrap();
        assert_ne!(head, head2);
        assert_eq!(
            list_tree(&dir, &head2, "notes").unwrap(),
            vec!["c3-third.md"]
        );
        assert!(
            resolve_rev(&dir, Some("side")).is_err(),
            "a dropped branch is pruned"
        );
        assert!(seconds_since_fetch(&dir).is_some());
        assert!(dir.join("FETCH_HEAD").exists());
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    #[test]
    fn tags_and_a_detached_head_mirror_into_the_slot() {
        let _env = crate::env_lock();
        let base = scratch("tags");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let repo = init(&origin);
        let c1 = commit_files(&repo, &[("n/a.md", "one\n")], "one");
        let c2 = commit_files(&repo, &[("n/a.md", "two\n")], "two");
        // A lightweight tag at c1 and an annotated tag at c2.
        let light: gix::refs::FullName = "refs/tags/v1".try_into().unwrap();
        repo.edit_reference(update_edit(light, gix::refs::Target::Object(c1)))
            .unwrap();
        let s = sig();
        repo.tag(
            "v2",
            c2,
            gix::objs::Kind::Commit,
            Some(s.to_ref(&mut gix::date::parse::TimeBuf::default())),
            "release two",
            gix::refs::transaction::PreviousValue::MustNotExist,
        )
        .unwrap();
        // Tags on a blob and on a tree: copied as objects, never walked.
        let blob_id = repo.write_blob(b"just bytes\n").unwrap().detach();
        let blob_tag: gix::refs::FullName = "refs/tags/blob-tag".try_into().unwrap();
        repo.edit_reference(update_edit(blob_tag, gix::refs::Target::Object(blob_id)))
            .unwrap();
        let tree_id = repo.find_commit(c1).unwrap().tree_id().unwrap().detach();
        let tree_tag: gix::refs::FullName = "refs/tags/tree-tag".try_into().unwrap();
        repo.edit_reference(update_edit(tree_tag, gix::refs::Target::Object(tree_id)))
            .unwrap();
        // A commit on no branch, with HEAD detached at it.
        let head: gix::refs::FullName = "HEAD".try_into().unwrap();
        repo.edit_reference(update_edit(head.clone(), gix::refs::Target::Object(c2)))
            .unwrap();
        let repo = gix::open_opts(&origin, gix::open::Options::isolated()).unwrap();
        let c3 = commit_files(&repo, &[("n/a.md", "three\n")], "three (detached)");
        assert!(
            repo.head_name().unwrap().is_none(),
            "the source HEAD is detached"
        );

        let url = format!("file://{}", origin.display());
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };
        let dir = ensure_clone(&url).unwrap();
        assert_eq!(
            resolve_rev(&dir, Some("v1")).unwrap(),
            c1.to_string(),
            "a lightweight tag"
        );
        assert_eq!(
            resolve_rev(&dir, Some("v2")).unwrap(),
            c2.to_string(),
            "an annotated tag peels to its commit"
        );
        assert_eq!(
            resolve_rev(&dir, None).unwrap(),
            c3.to_string(),
            "HEAD follows the detached source"
        );
        assert_eq!(show(&dir, "HEAD", "n/a.md").unwrap(), "three\n");
        let slot = open_isolated(&dir).unwrap();
        assert!(
            slot.find_object(blob_id).is_ok(),
            "a blob tag's object is in the slot"
        );
        assert!(
            slot.find_object(tree_id).is_ok(),
            "a tree tag's object is in the slot"
        );
        assert!(slot.find_reference("refs/tags/blob-tag").is_ok());
        assert!(
            resolve_rev(&dir, Some("blob-tag")).is_err(),
            "a blob tag is not a commit"
        );
        // A later fetch keeps the same properties, and a tag the source
        // dropped is pruned.
        let v1: gix::refs::FullName = "refs/tags/v1".try_into().unwrap();
        repo.edit_reference(delete_edit(v1)).unwrap();
        let orphan = dir.with_extension("tmp-424242");
        std::fs::create_dir_all(&orphan).unwrap();
        fetch(&url).unwrap();
        assert_eq!(resolve_rev(&dir, Some("v2")).unwrap(), c2.to_string());
        assert!(
            resolve_rev(&dir, Some("v1")).is_err(),
            "a deleted tag is pruned"
        );
        assert!(!orphan.exists(), "an orphan staging dir is swept");
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    #[test]
    fn a_slot_made_by_the_git_cli_still_syncs() {
        // `git clone --bare` writes a config with a url and no fetch
        // line. The refspec must come from mdstore, not the slot.
        let _env = crate::env_lock();
        let base = scratch("cli-slot");
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let repo = init(&origin);
        let c1 = commit_files(&repo, &[("a.md", "one\n")], "one");
        let url = format!("file://{}", origin.display());
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };
        let dir = ensure_clone(&url).unwrap();
        // Rewrite the slot config the way the CLI leaves it.
        std::fs::write(
            dir.join("config"),
            format!("[core]\n\tbare = true\n[remote \"origin\"]\n\turl = {url}\n"),
        )
        .unwrap();
        let c2 = commit_files(&repo, &[("a.md", "two\n")], "two");
        fetch(&url).unwrap();
        assert_eq!(resolve_rev(&dir, None).unwrap(), c2.to_string());
        assert_ne!(c1, c2);
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }

    /// The network path, against a public repository. Ignored by
    /// default because it needs the network; run with
    /// `cargo test -- --ignored network` before a release.
    #[test]
    #[ignore = "needs the network"]
    fn network_clone_and_fetch_over_https_run_in_process() {
        let _env = crate::env_lock();
        let base = scratch("net");
        // A public repository with tags, so packed refs and Tags::All
        // are both exercised.
        let url = "https://github.com/BurntSushi/termcolor";
        unsafe { std::env::set_var("MDSTORE_CACHE_DIR", base.join("cache")) };
        let dir = ensure_clone(url).unwrap();
        assert!(
            is_cached(url),
            "the slot opens after a clone that packed its refs"
        );
        assert!(
            dir.join("refs").is_dir(),
            "refs/ exists even when every ref is packed"
        );
        let head = resolve_rev(&dir, None).unwrap();
        assert!(resolve_rev(&dir, Some("master")).is_ok());
        assert!(
            resolve_rev(&dir, Some("1.0.0")).is_ok(),
            "a tag pin resolves"
        );
        assert!(
            list_tree(&dir, &head, "src")
                .unwrap()
                .iter()
                .any(|f| f == "lib.rs")
        );
        assert!(
            show(&dir, &head, "Cargo.toml")
                .unwrap()
                .contains("[package]")
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("HEAD")).unwrap().trim(),
            "ref: refs/heads/master"
        );
        // A slot config the CLI would leave: url only. Fetch still works.
        std::fs::write(
            dir.join("config"),
            format!("[core]\n\tbare = true\n[remote \"origin\"]\n\turl = {url}\n"),
        )
        .unwrap();
        fetch(url).unwrap();
        assert_eq!(resolve_rev(&dir, None).unwrap(), head);
        assert!(dir.join("FETCH_HEAD").exists());
        assert!(is_cached(url), "the slot opens after a fetch too");
        // HEAD follows the remote: point the slot's HEAD elsewhere, and
        // a fetch puts it back from the advertised symref.
        std::fs::write(dir.join("HEAD"), "ref: refs/heads/no-such-branch\n").unwrap();
        fetch(url).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("HEAD")).unwrap().trim(),
            "ref: refs/heads/master"
        );
        assert!(
            resolve_rev(&dir, Some("1.0.0")).is_ok(),
            "tags survive a fetch"
        );
        unsafe { std::env::remove_var("MDSTORE_CACHE_DIR") };
    }
}
