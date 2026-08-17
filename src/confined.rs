//! A store directory this process may read and write, and nothing
//! above it.
//!
//! # Why a handle rather than a predicate
//!
//! A store joins caller text onto a directory to build a file path.
//! The text is not trusted: a note id, an issue id, a skill name, and
//! a project name all arrive from a person, from a published store, or
//! from the network. Guarding that with a predicate puts the burden on
//! every caller to remember the predicate, and four rounds of review
//! found a different caller that did not.
//!
//! [`StoreDir`] holds an open directory. Every read, write, and scan
//! goes through the handle, and the operating system refuses a path
//! that leaves it. A caller cannot forget the check, because there is
//! no check to forget.
//!
//! # What this does not cover
//!
//! A store whose content lives in a git tree reads through the object
//! database, so no path is built from third-party text and nothing
//! here applies. The bare clone that backs such a store lives at a
//! location mdstore chooses, not one a dependency names.

use std::path::{Path, PathBuf};

use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};

use crate::error::{Error, Result};
use crate::store::{Scan, ScanEntry};

/// One store's directory, and the authority to read and write inside
/// it.
///
/// A clone shares the open directory rather than opening the root
/// again. Re-opening per operation would resolve the root through
/// ambient authority every time, so a root swapped between two calls
/// would be followed. Sharing one handle is also what makes a scan of
/// 500 documents one root open rather than 501.
#[derive(Debug, Clone)]
pub struct StoreDir {
    /// Where the store is, for a message or an identity. Never used to
    /// build a path for an operation.
    root: PathBuf,
    dir: std::sync::Arc<Dir>,
}

/// Two handles are the same store when they name the same root.
impl PartialEq for StoreDir {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}

impl Eq for StoreDir {}

impl StoreDir {
    /// Open a store root.
    ///
    /// The root itself is resolved with the authority this process
    /// already holds, because a person may keep a store behind a link
    /// and name it on the command line. Everything under the root is
    /// then confined to it.
    pub fn open(root: &Path) -> Result<Self> {
        let dir = Dir::open_ambient_dir(root, ambient_authority()).map_err(|e| {
            Error::InvalidStore(format!("cannot open store at {}: {e}", root.display()))
        })?;
        Ok(StoreDir {
            root: root.to_path_buf(),
            dir: std::sync::Arc::new(dir),
        })
    }

    /// The name of the store root itself.
    ///
    /// An empty relative path names the root to every caller, but the
    /// operating system refuses it. Reading it as "." made a scan of
    /// the root return no documents, and the NotFound arm hid that as
    /// an empty store.
    /// True when the path names a parent at any point.
    ///
    /// A climb may still land inside the store, and the handle decides
    /// that. This only says the question was asked, which is enough to
    /// tell a missing directory from a refused escape.
    fn climbs(rel: &str) -> bool {
        Path::new(rel)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    }

    fn at(rel: &str) -> &str {
        if rel.is_empty() { "." } else { rel }
    }

    /// Where this store is on this machine.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read one document.
    ///
    /// A path that leaves the store fails. A file that is not a
    /// regular file is refused by type rather than opened, so a link,
    /// a directory, and a device are all errors.
    pub fn read(&self, rel: &str) -> Result<String> {
        let meta = self
            .dir
            .symlink_metadata(rel)
            .map_err(|e| self.io_error(rel, &e))?;
        if meta.is_symlink() {
            return Err(Error::InvalidStore(format!(
                "{rel} is a symlink and is not read"
            )));
        }
        if !meta.is_file() {
            return Err(Error::InvalidStore(format!("{rel} is not a regular file")));
        }
        self.dir
            .read_to_string(rel)
            .map_err(|e| self.io_error(rel, &e))
    }

    /// True when a regular file sits at this path.
    #[must_use]
    pub fn is_document(&self, rel: &str) -> bool {
        self.dir
            .symlink_metadata(rel)
            .is_ok_and(|m| m.is_file() && !m.is_symlink())
    }

    /// Write one document, replacing what was there.
    ///
    /// The write stages into a uniquely named temporary file created
    /// with `O_EXCL`, then renames it over the destination. The
    /// exclusive create refuses a temporary path something else
    /// planted, and the rename means a reader sees the old document or
    /// the new one and never a half-written one.
    pub fn write(&self, rel: &str, contents: &str) -> Result<()> {
        let parent = Path::new(rel).parent().unwrap_or(Path::new(""));
        let name = Path::new(rel)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::InvalidStore(format!("{rel} has no file name")))?;

        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.write_staged(rel, contents, parent, name, unique)
    }

    /// The staging name one write uses.
    ///
    /// The counter is a parameter so a test can plant a file at the
    /// exact name a write will take. A test that plants a guess
    /// against a process-global counter asserts nothing: every earlier
    /// write in the process has already moved it past the guess.
    fn stage_name(parent: &Path, name: &str, unique: u64) -> String {
        parent
            .join(format!(".{name}.{}.{unique}.tmp", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    fn write_staged(
        &self,
        rel: &str,
        contents: &str,
        parent: &Path,
        name: &str,
        unique: u64,
    ) -> Result<()> {
        use std::io::Write as _;

        let temp = Self::stage_name(parent, name, unique);

        // The exclusive create is the point where this call takes
        // ownership of the staging name. Before it succeeds the file
        // belongs to whoever else is mid-write there, and the error
        // path must not remove it. Removing it destroyed that writer's
        // staged content.
        let mut file = match self
            .dir
            .open_with(&temp, OpenOptions::new().write(true).create_new(true))
        {
            Ok(file) => file,
            Err(e) => return Err(self.io_error(&temp, &e)),
        };

        let staged = (|| -> std::io::Result<()> {
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        })();
        if let Err(e) = staged {
            let _ = self.dir.remove_file(&temp);
            return Err(self.io_error(&temp, &e));
        }

        if let Err(e) = self.dir.rename(&temp, &self.dir, rel) {
            let _ = self.dir.remove_file(&temp);
            return Err(self.io_error(rel, &e));
        }
        Ok(())
    }

    /// True when the name exists inside the store but is not a
    /// regular file.
    ///
    /// A scan skips a link by type, so this is what tells a caller
    /// that something was there and was refused. A name that leaves
    /// the store is not present, so this answers false rather than
    /// reaching out to follow it.
    #[must_use]
    pub fn present_but_irregular(&self, rel: &str) -> bool {
        self.dir
            .symlink_metadata(rel)
            .is_ok_and(|m| !m.file_type().is_file())
    }

    /// Remove one document.
    pub fn remove(&self, rel: &str) -> Result<()> {
        self.dir
            .remove_file(rel)
            .map_err(|e| self.io_error(rel, &e))
    }

    /// Move one entry to another name inside the store.
    ///
    /// Both names go through the handle, so neither end can leave the
    /// store. The move is one operation, so a reader sees the entry at
    /// the old name or the new one and never at neither. A consumer
    /// that moved a document by reading, writing and removing left a
    /// copy at both names when the remove failed.
    ///
    /// # This destroys an existing destination
    ///
    /// An entry already at `to` is replaced, and what it held is gone.
    /// No error is raised.
    ///
    /// The operating system can refuse atomically: Linux has
    /// `renameat2(RENAME_NOREPLACE)` and macOS has
    /// `renameatx_np(RENAME_EXCL)`. cap-std exposes neither, so a
    /// refusal here could only test the name first, and that check can
    /// go stale between the test and the move. A handle exists so that there is
    /// no such check to get wrong, so the primitive is not wrapped in
    /// one here.
    ///
    /// A consumer that must not lose a document needs a destination
    /// that cannot collide. Testing the name first has the same stale
    /// window and only looks safer.
    ///
    /// # A directory moves too
    ///
    /// A directory moves with everything under it, and replaces an
    /// empty directory at `to`. The name says entry for that reason.
    pub fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.dir
            .rename(from, &self.dir, to)
            .map_err(|e| self.io_error(from, &e))
    }

    /// Remove one empty directory inside the store.
    ///
    /// The store root itself is refused. An empty root answers true to
    /// [`Self::dir_is_empty`] and errors here, which is the one input
    /// where that pair disagrees: a store cannot unlink its own root
    /// through its own handle.
    pub fn remove_dir(&self, rel: &str) -> Result<()> {
        self.dir
            .remove_dir(Self::at(rel))
            .map_err(|e| self.io_error(rel, &e))
    }

    /// True when the directory holds no entry at all.
    ///
    /// Every entry counts, including a dotfile and a name that is not
    /// valid UTF-8. A caller that removes a directory it believes is
    /// empty must not be told so by a listing that filtered.
    ///
    /// A path that leaves the store, and a path with no directory, are
    /// both not empty. Neither is a directory this store may remove.
    #[must_use]
    pub fn dir_is_empty(&self, rel: &str) -> bool {
        self.dir
            .read_dir(Self::at(rel))
            .is_ok_and(|mut entries| entries.next().is_none())
    }

    /// Create a directory and its parents inside the store.
    pub fn create_dir_all(&self, rel: &str) -> Result<()> {
        self.dir
            .create_dir_all(rel)
            .map_err(|e| self.io_error(rel, &e))
    }

    /// The subdirectory names under one directory of the store.
    ///
    /// A link is skipped because the dirent type of a link is a link,
    /// not a directory, and this never follows it. Names starting with
    /// a dot are omitted. The result is sorted.
    ///
    /// A path that leaves the store lists nothing. This signature
    /// carries no refusal, so the escape is empty rather than an
    /// error; `scan` and `read` refuse the same path outright.
    #[must_use]
    pub fn subdirectories(&self, rel: &str) -> Vec<String> {
        let Ok(entries) = self.dir.read_dir(Self::at(rel)) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().to_str().map(ToString::to_string))
            .filter(|n| !n.starts_with('.'))
            .collect();
        names.sort();
        names
    }

    /// The `.md` files of one directory, with their stems.
    ///
    /// Every entry the scan refuses is recorded with its reason, so a
    /// consumer's `check` command can name it. A skipped file is never
    /// silent.
    pub fn scan(&self, rel: &str) -> Result<Scan> {
        let mut scan = Scan::default();
        let entries = match self.dir.read_dir(Self::at(rel)) {
            Ok(entries) => entries,
            // A store with no document directory yet holds no
            // documents. That is not a failure.
            //
            // A climbing path is a different thing. When a leading
            // component is missing, the operating system answers
            // NotFound before it evaluates the climb, so this arm
            // turned a refusal into a silent empty result. A path that
            // tries to leave is refused whether or not it arrives.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !Self::climbs(rel) => {
                return Ok(scan);
            }
            Err(e) => return Err(self.io_error(rel, &e)),
        };

        let mut found: Vec<(PathBuf, String)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                scan.skipped.push((
                    Path::new(rel).join(entry.file_name()),
                    "name is not valid UTF-8".to_string(),
                ));
                continue;
            };
            let path = Path::new(rel).join(name);
            if Path::new(name).extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            // The dirent type does not follow a link.
            let Ok(file_type) = entry.file_type() else {
                scan.skipped.push((path, "type is unreadable".to_string()));
                continue;
            };
            if file_type.is_symlink() {
                scan.skipped
                    .push((path, "symlink (not followed)".to_string()));
                continue;
            }
            if !file_type.is_file() {
                scan.skipped.push((path, "not a regular file".to_string()));
                continue;
            }
            let stem = Path::new(name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(name)
                .to_string();
            found.push((path, stem));
        }
        found.sort();
        scan.entries = found
            .into_iter()
            .map(|(path, stem)| ScanEntry { path, stem })
            .collect();
        Ok(scan)
    }

    /// One error shape, so a message never leaks a path outside the
    /// store and always names the store it refers to.
    fn io_error(&self, rel: &str, e: &std::io::Error) -> Error {
        Error::StorePath {
            rel: rel.to_string(),
            root: self.root.display().to_string(),
            // io::Error is not Clone, so it is rebuilt. The errno is
            // carried when there is one, because that is what tells a
            // refusal from a fault: cap-std refuses an escaping path
            // with its own message and no errno, while a real denial
            // carries EACCES. Rebuilding from the kind alone lost that
            // and made the two identical.
            source: e.raw_os_error().map_or_else(
                || std::io::Error::new(e.kind(), e.to_string()),
                std::io::Error::from_raw_os_error,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store with one note, plus a directory outside it holding a
    /// file that must stay untouched.
    struct Fixture {
        base: PathBuf,
        store: StoreDir,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let base =
                std::env::temp_dir().join(format!("mdstore-confined-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(base.join("store/docs")).unwrap();
            std::fs::create_dir_all(base.join("outside")).unwrap();
            std::fs::write(base.join("outside/secret.md"), "SECRET").unwrap();
            std::fs::write(base.join("store/docs/note.md"), "a note").unwrap();
            let store = StoreDir::open(&base.join("store")).unwrap();
            Fixture { base, store }
        }

        fn outside_is_intact(&self) -> bool {
            std::fs::read_to_string(self.base.join("outside/secret.md"))
                .is_ok_and(|s| s == "SECRET")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    // -- escape through a document name --

    #[test]
    fn a_name_that_climbs_out_reads_nothing() {
        let f = Fixture::new("climb");
        for name in [
            "../outside/secret.md",
            "../../outside/secret.md",
            "docs/../../outside/secret.md",
            "docs/a/b/../../../../outside/secret.md",
        ] {
            assert!(f.store.read(name).is_err(), "{name} must not read");
            assert!(!f.store.is_document(name), "{name} must not exist");
        }
        assert!(f.outside_is_intact());
    }

    #[test]
    fn an_absolute_name_reads_nothing() {
        let f = Fixture::new("abs");
        let absolute = f.base.join("outside/secret.md");
        let name = absolute.to_string_lossy();
        assert!(f.store.read(&name).is_err());
        assert!(f.store.read("/etc/passwd").is_err());
        assert!(f.outside_is_intact());
    }

    #[test]
    fn a_dot_name_is_not_a_document() {
        let f = Fixture::new("dots");
        for name in ["..", ".", "docs/..", "docs/."] {
            assert!(f.store.read(name).is_err(), "{name}");
            assert!(!f.store.is_document(name), "{name}");
        }
    }

    #[test]
    fn a_name_with_a_nul_byte_is_refused() {
        let f = Fixture::new("nul");
        assert!(f.store.read("docs/note\0.md").is_err());
        assert!(f.store.write("docs/note\0.md", "x").is_err());
    }

    #[test]
    fn a_backslash_name_reads_nothing_outside() {
        // A backslash is an ordinary character on this platform, so
        // the name simply does not exist. It must never resolve out.
        let f = Fixture::new("backslash");
        assert!(f.store.read("..\\outside\\secret.md").is_err());
        assert!(f.outside_is_intact());
    }

    #[test]
    fn a_unicode_separator_lookalike_is_not_a_separator() {
        // U+2215 DIVISION SLASH looks like a separator and is not one,
        // so it can only ever be part of one file name.
        let f = Fixture::new("lookalike");
        assert!(f.store.read("..\u{2215}outside\u{2215}secret.md").is_err());
        assert!(f.outside_is_intact());
    }

    #[test]
    fn a_percent_encoded_climb_is_one_name() {
        let f = Fixture::new("percent");
        assert!(f.store.read("%2e%2e%2foutside%2fsecret.md").is_err());
        assert!(f.outside_is_intact());
    }

    // -- escape through the filesystem --

    #[test]
    fn a_linked_document_is_refused_by_type() {
        let f = Fixture::new("linkdoc");
        std::os::unix::fs::symlink(
            f.base.join("outside/secret.md"),
            f.base.join("store/docs/linked.md"),
        )
        .unwrap();

        let err = f.store.read("docs/linked.md").unwrap_err();
        assert!(format!("{err}").contains("symlink"), "{err}");
        assert!(!f.store.is_document("docs/linked.md"));

        // The scan names it rather than dropping it silently.
        let scan = f.store.scan("docs").unwrap();
        assert!(scan.entries.iter().all(|e| e.stem != "linked"));
        assert!(
            scan.skipped.iter().any(|(p, why)| {
                p.to_string_lossy().contains("linked") && why.contains("symlink")
            })
        );
    }

    #[test]
    fn a_linked_directory_is_not_walked() {
        let f = Fixture::new("linkdir");
        std::os::unix::fs::symlink(f.base.join("outside"), f.base.join("store/linked")).unwrap();

        assert!(f.store.subdirectories("").iter().all(|n| n != "linked"));
        assert!(f.store.read("linked/secret.md").is_err());

        // Every method that takes a name, not only the readers. A
        // guard written on a parent component and a leading slash
        // answers all of these wrong, because a link carries neither.
        assert!(f.store.rename("docs/note.md", "linked/stolen.md").is_err());
        assert!(f.store.rename("linked/secret.md", "docs/taken.md").is_err());
        assert!(!f.store.is_document("linked/secret.md"));
        assert!(f.store.remove("linked/secret.md").is_err());
        assert!(f.store.scan("linked").is_err());

        // An empty directory through the link, so the refusal cannot
        // be mistaken for ENOTEMPTY.
        std::fs::create_dir_all(f.base.join("outside/hollow")).unwrap();
        assert!(!f.store.dir_is_empty("linked/hollow"));
        assert!(f.store.remove_dir("linked/hollow").is_err());
        assert!(
            f.base.join("outside/hollow").is_dir(),
            "a directory outside the store was removed through a link"
        );

        assert!(f.outside_is_intact());
        assert!(!f.store.is_document("docs/taken.md"));
        assert_eq!(f.store.read("docs/note.md").unwrap(), "a note");
    }

    #[test]
    fn a_write_cannot_be_captured_by_a_planted_staging_link() {
        // The capability refuses this, not the exclusive create: an
        // open through a link that leaves the store fails whatever
        // flags it carries. Proven by the sibling test below, which
        // shows a plain create fails the same way.
        let f = Fixture::new("staging");
        // The exact staging name the write will take. Guessing at a
        // process-global counter proves nothing, because every earlier
        // write in the process has already moved it.
        let planted = StoreDir::stage_name(Path::new("docs"), "note.md", 7);
        std::os::unix::fs::symlink(
            f.base.join("outside/secret.md"),
            f.base.join("store").join(&planted),
        )
        .unwrap();

        let result = f.store.write_staged(
            "docs/note.md",
            "new content",
            Path::new("docs"),
            "note.md",
            7,
        );
        assert!(result.is_err(), "a write onto a planted link must fail");

        assert!(f.outside_is_intact(), "the victim file was written");
        let meta = std::fs::symlink_metadata(f.base.join("store/docs/note.md")).unwrap();
        assert!(!meta.file_type().is_symlink(), "the document became a link");
    }

    #[test]
    fn a_plain_create_through_an_escaping_link_also_fails() {
        // What the handle gives that a flag cannot: the escape is
        // refused by the operating system, so a caller that forgets
        // O_EXCL still cannot write outside the store.
        let f = Fixture::new("plaincreate");
        std::os::unix::fs::symlink(
            f.base.join("outside/secret.md"),
            f.base.join("store/docs/planted.tmp"),
        )
        .unwrap();
        let opened = f.store.dir.open_with(
            "docs/planted.tmp",
            OpenOptions::new().write(true).create(true).truncate(true),
        );
        assert!(opened.is_err(), "an escaping link must not open");
        assert!(f.outside_is_intact());
    }

    #[test]
    fn an_exclusive_create_refuses_a_staging_file_already_there() {
        // What O_EXCL still gives, now that escape is handled: two
        // writers inside one store never share a staging file, so
        // neither truncates the other's half-written document.
        let f = Fixture::new("exclusive");
        f.store.write("docs/taken.md", "first").unwrap();
        // Plant at the name the second write will take, then drive the
        // real write. Calling the raw open instead of `write` is why
        // this test missed that the error path deleted the file.
        let planted = StoreDir::stage_name(Path::new("docs"), "taken.md", 3);
        std::fs::write(f.base.join("store").join(&planted), "in progress").unwrap();

        let again =
            f.store
                .write_staged("docs/taken.md", "second", Path::new("docs"), "taken.md", 3);
        assert!(
            again.is_err(),
            "an exclusive create must refuse a taken name"
        );
        assert_eq!(
            std::fs::read_to_string(f.base.join("store").join(&planted)).unwrap(),
            "in progress",
            "the other writer's staging file was destroyed"
        );
        assert_eq!(
            f.store.read("docs/taken.md").unwrap(),
            "first",
            "the document changed despite the failed write"
        );
    }

    #[test]
    fn a_fifo_is_not_a_document() {
        let f = Fixture::new("fifo");
        let fifo = f.base.join("store/docs/pipe.md");
        let out = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !out {
            return; // mkfifo is unavailable; nothing to assert
        }
        assert!(!f.store.is_document("docs/pipe.md"));
        let scan = f.store.scan("docs").unwrap();
        assert!(scan.entries.iter().all(|e| e.stem != "pipe"));
        assert!(
            scan.skipped
                .iter()
                .any(|(p, _)| p.to_string_lossy().contains("pipe"))
        );

        // The reason the regular-file guard exists. Opening a FIFO to
        // read blocks until a writer arrives, and no writer is coming,
        // so a read without the guard never returns. The read runs on
        // its own thread because a hang here would hang the suite.
        let store = f.store.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(store.read("docs/pipe.md").is_err());
        });
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(refused) => assert!(refused, "a fifo was read as a document"),
            Err(_) => panic!("read on a fifo blocked; the regular-file guard is gone"),
        }
    }

    /// The reason the handle is held rather than re-derived.
    ///
    /// A path re-opened per operation resolves through ambient
    /// authority every time. Swapping the root between two reads then
    /// redirects the second one. An open handle names the directory
    /// itself, so the swap has nothing to catch.
    #[test]
    fn a_swapped_root_does_not_redirect_an_open_store() {
        let f = Fixture::new("swap");
        assert_eq!(f.store.read("docs/note.md").unwrap(), "a note");

        // Move the real store aside and put a link to a decoy at the
        // name the store was opened under.
        std::fs::create_dir_all(f.base.join("decoy/docs")).unwrap();
        std::fs::write(f.base.join("decoy/docs/note.md"), "SWAPPED").unwrap();
        std::fs::rename(f.base.join("store"), f.base.join("store-moved")).unwrap();
        std::os::unix::fs::symlink(f.base.join("decoy"), f.base.join("store")).unwrap();

        assert_eq!(
            f.store.read("docs/note.md").unwrap(),
            "a note",
            "the swapped root redirected a read on an open store"
        );

        f.store.write("docs/note.md", "written").unwrap();
        assert_eq!(
            std::fs::read_to_string(f.base.join("store-moved/docs/note.md")).unwrap(),
            "written",
            "a write landed outside the store the handle was opened on"
        );
        assert_eq!(
            std::fs::read_to_string(f.base.join("decoy/docs/note.md")).unwrap(),
            "SWAPPED",
            "a write reached the decoy"
        );
    }

    #[test]
    fn a_move_cannot_take_a_file_out_of_the_store_or_bring_one_in() {
        let f = Fixture::new("rename");
        assert!(
            f.store
                .rename("docs/note.md", "../outside/stolen.md")
                .is_err(),
            "a move carried a document out of the store"
        );
        assert!(
            f.store
                .rename("../outside/secret.md", "docs/taken.md")
                .is_err(),
            "a move brought a file in from outside the store"
        );
        assert!(f.outside_is_intact());
        assert_eq!(f.store.read("docs/note.md").unwrap(), "a note");

        // A move inside the store works, and leaves nothing behind.
        f.store.create_dir_all("docs/.closed").unwrap();
        f.store
            .rename("docs/note.md", "docs/.closed/note.md")
            .unwrap();
        assert_eq!(f.store.read("docs/.closed/note.md").unwrap(), "a note");
        assert!(!f.store.is_document("docs/note.md"));

        // The refused inbound move must not have created anything.
        assert!(!f.store.is_document("docs/taken.md"));
    }

    /// A move replaces what sat at the destination, and raises no
    /// error. Pinned on its own, because it is a contract a consumer
    /// looks for by name, and it does not belong under a confinement
    /// claim.
    #[test]
    fn a_move_replaces_what_sat_at_the_destination() {
        let f = Fixture::new("replace");
        f.store.write("docs/a.md", "AAA").unwrap();
        f.store.write("docs/b.md", "BBB").unwrap();
        f.store.rename("docs/a.md", "docs/b.md").unwrap();
        assert_eq!(f.store.read("docs/b.md").unwrap(), "AAA", "BBB survived");
        assert!(!f.store.is_document("docs/a.md"), "the source stayed");

        // A directory replaces an empty directory the same way, and
        // the changelog advertises that a directory moves.
        f.store.create_dir_all("docs/src").unwrap();
        f.store.write("docs/src/kept.md", "KEPT").unwrap();
        f.store.create_dir_all("docs/dst").unwrap();
        f.store.rename("docs/src", "docs/dst").unwrap();
        assert_eq!(f.store.read("docs/dst/kept.md").unwrap(), "KEPT");
        assert!(!f.store.dir_is_empty("docs/dst"));
    }

    #[test]
    fn an_empty_directory_is_told_apart_from_one_holding_a_dotfile() {
        let f = Fixture::new("empty");
        f.store.create_dir_all("docs/.closed").unwrap();
        assert!(f.store.dir_is_empty("docs/.closed"));

        // A dotfile is an entry. A listing that filtered dot names
        // would call this empty and the remove would then fail.
        std::fs::write(f.base.join("store/docs/.closed/.keep"), "").unwrap();
        assert!(!f.store.dir_is_empty("docs/.closed"));
        assert!(f.store.remove_dir("docs/.closed").is_err());

        std::fs::remove_file(f.base.join("store/docs/.closed/.keep")).unwrap();
        assert!(f.store.dir_is_empty("docs/.closed"));
        f.store.remove_dir("docs/.closed").unwrap();

        // A path that leaves the store is never empty and never
        // removed. The directory outside must be EMPTY, or both
        // assertions pass on ENOTEMPTY and prove nothing: an ambient
        // remove_dir would be refused for holding a file, not for
        // leaving the store, and an ambient read_dir would report the
        // file rather than the refusal.
        let victim = f.base.join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        assert!(!f.store.dir_is_empty("../victim"));
        assert!(f.store.remove_dir("../victim").is_err());
        assert!(
            victim.is_dir(),
            "an empty directory outside the store was removed"
        );

        // The same directory by an absolute path. A guard written on
        // '..' alone answers this one wrong: an absolute path names
        // the same directory and carries no parent component.
        let abs = victim.to_string_lossy().into_owned();
        assert!(
            !f.store.dir_is_empty(&abs),
            "an absolute path outside the store was called empty"
        );
        assert!(
            f.store.remove_dir(&abs).is_err(),
            "an absolute path outside the store was removed"
        );
        assert!(
            victim.is_dir(),
            "the directory named absolutely was removed"
        );

        // The store's own empty directory is still told apart, so the
        // assertion above is about the escape and not about emptiness.
        f.store.create_dir_all("docs/inside").unwrap();
        assert!(f.store.dir_is_empty("docs/inside"));
        f.store.remove_dir("docs/inside").unwrap();
        assert!(!f.base.join("store/docs/inside").exists());
        assert!(f.base.join("store/docs").is_dir(), "the parent went too");
        assert!(f.outside_is_intact());
    }

    /// A refusal and a fault must not read the same to a consumer.
    ///
    /// The error carried only a string, so a directory the store
    /// refuses and a directory it cannot read were the same value. A
    /// consumer that treated the first as an empty store then treated
    /// a mode-000 directory as empty too, and answered zero documents
    /// with a success status.
    #[test]
    fn a_refusal_and_a_permissions_failure_carry_different_kinds() {
        let f = Fixture::new("kinds");

        // A path that leaves the store. cap-std refuses it.
        let escaped = f.store.scan("../outside").unwrap_err();
        assert!(
            escaped.refused_by_confinement(),
            "an escape did not read as a refusal: {escaped}"
        );

        // A directory inside the store that cannot be read.
        f.store.create_dir_all("docs/locked").unwrap();
        let locked = f.base.join("store/docs/locked");
        let mut perms = std::fs::metadata(&locked).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o000);
        std::fs::set_permissions(&locked, perms).unwrap();

        // Same kind as the escape. Only the errno separates them, so
        // a consumer keying on the kind treats a locked directory as
        // an empty one.
        let denied = f.store.scan("docs/locked").unwrap_err();
        assert_eq!(denied.io_kind(), Some(std::io::ErrorKind::PermissionDenied));
        assert!(
            !denied.refused_by_confinement(),
            "a permissions failure read as a refusal: {denied}"
        );

        // A name that is a file, not a directory. A store holds no
        // documents there, and that is not a fault.
        let wrong = f.store.scan("docs/note.md").unwrap_err();
        assert_eq!(wrong.io_kind(), Some(std::io::ErrorKind::NotADirectory));
        assert!(!wrong.refused_by_confinement());

        // An errno-less error of another kind is not a refusal either.
        // The predicate checks the kind as well as the errno, and
        // dropping the kind check left every test green: nothing fed
        // it an errno-less non-PermissionDenied. cap-std produces one
        // when create_dir_all cannot make the whole tree.
        let synthetic = Error::StorePath {
            rel: "x".into(),
            root: "y".into(),
            source: std::io::Error::other("failed to create whole tree"),
        };
        assert!(
            !synthetic.refused_by_confinement(),
            "an errno-less Other read as a refusal"
        );

        let mut perms = std::fs::metadata(&locked).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&locked, perms).unwrap();
    }

    // -- legitimate use --

    #[test]
    fn an_ordinary_document_round_trips() {
        let f = Fixture::new("roundtrip");
        assert_eq!(f.store.read("docs/note.md").unwrap(), "a note");

        f.store.write("docs/note.md", "edited").unwrap();
        assert_eq!(f.store.read("docs/note.md").unwrap(), "edited");

        f.store.write("docs/fresh.md", "new").unwrap();
        assert_eq!(f.store.read("docs/fresh.md").unwrap(), "new");

        f.store.remove("docs/fresh.md").unwrap();
        assert!(!f.store.is_document("docs/fresh.md"));
    }

    #[test]
    fn a_write_leaves_no_temporary_file() {
        let f = Fixture::new("notemp");
        f.store.write("docs/note.md", "edited").unwrap();
        let leftovers: Vec<String> = std::fs::read_dir(f.base.join("store/docs"))
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(ToString::to_string))
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn a_document_in_a_subdirectory_works() {
        // Tisket keeps issues under one directory for each project.
        let f = Fixture::new("subdir");
        f.store.create_dir_all("docs/project").unwrap();
        f.store.write("docs/project/issue.md", "an issue").unwrap();
        assert_eq!(f.store.read("docs/project/issue.md").unwrap(), "an issue");
        assert_eq!(f.store.subdirectories("docs"), vec!["project".to_string()]);
    }

    #[test]
    fn a_store_root_behind_a_link_still_opens() {
        // A person may keep a store behind a link and name it. That
        // is their own authority, and it must keep working.
        let f = Fixture::new("rootlink");
        let linked_root = f.base.join("by-link");
        std::os::unix::fs::symlink(f.base.join("store"), &linked_root).unwrap();

        let store = StoreDir::open(&linked_root).unwrap();
        assert_eq!(store.read("docs/note.md").unwrap(), "a note");
    }

    #[test]
    fn a_scan_sorts_and_reports_nothing_extra() {
        let f = Fixture::new("scan");
        f.store.write("docs/beta.md", "b").unwrap();
        f.store.write("docs/alpha.md", "a").unwrap();
        f.store.write("docs/notes.txt", "ignored").unwrap();

        let scan = f.store.scan("docs").unwrap();
        let stems: Vec<&str> = scan.entries.iter().map(|e| e.stem.as_str()).collect();
        assert_eq!(stems, vec!["alpha", "beta", "note"]);
        assert!(scan.skipped.is_empty(), "{:?}", scan.skipped);
    }

    // -- failure behaviour --

    #[test]
    fn a_missing_document_directory_holds_no_documents() {
        let f = Fixture::new("nodir");
        let scan = f.store.scan("absent").unwrap();
        assert!(scan.entries.is_empty());
        assert!(scan.skipped.is_empty());
    }

    #[test]
    fn a_missing_store_root_is_an_error_not_a_panic() {
        let missing = std::env::temp_dir().join("mdstore-confined-absent-root");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(StoreDir::open(&missing).is_err());
    }

    #[test]
    fn a_store_root_that_is_a_file_is_an_error() {
        let f = Fixture::new("rootfile");
        assert!(StoreDir::open(&f.base.join("store/docs/note.md")).is_err());
    }

    #[test]
    fn one_unreadable_document_leaves_the_rest_readable() {
        let f = Fixture::new("badbytes");
        std::fs::write(f.base.join("store/docs/bad.md"), [0xff, 0xfe]).unwrap();

        assert!(f.store.read("docs/bad.md").is_err());
        assert_eq!(f.store.read("docs/note.md").unwrap(), "a note");
        // The scan still lists both: the bytes are a read-time
        // problem, not a dirent-type one.
        let scan = f.store.scan("docs").unwrap();
        assert_eq!(scan.entries.len(), 2);
    }
}
