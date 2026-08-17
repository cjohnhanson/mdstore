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
        use std::io::Write as _;

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
    #[must_use]
    pub fn subdirectories(&self, rel: &str) -> Vec<String> {
        let Ok(entries) = self.dir.read_dir(rel) else {
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
        let entries = match self.dir.read_dir(rel) {
            Ok(entries) => entries,
            // A store with no document directory yet holds no
            // documents. That is not a failure.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(scan),
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
        Error::InvalidStore(format!("{rel} in {}: {e}", self.root.display()))
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
        assert!(f.outside_is_intact());
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
