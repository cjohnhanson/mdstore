//! Key bindings read from a file, over a built-in preset.
//!
//! A tool names its commands with an enum that implements [`Command`],
//! and supplies each preset's bindings for that enum as a [`Section`].
//! [`KeysFile`] reads the user's file, picks the preset, and lays the
//! user's bindings over it. The result is a [`Keymap`], which turns key
//! events into commands, holds a pending prefix such as `C-x`, and
//! lists the bindings for a help screen. The same table does all
//! three, so no key works without being listed.
//!
//! The file is YAML:
//!
//! ```yaml
//! format: 1
//! preset: emacs        # default | emacs
//! editor:
//!   "C-c C-c": save-buffer
//!   "M-q": unbound     # removes the preset's binding
//! list:
//!   "C-x C-c": quit
//! ```
//!
//! Each tool reads its own file, `~/.config/<tool>/keys.yml`, beside the
//! user config `~/.config/<tool>/config.yml` (see `userconfig`). It is a
//! separate file because the user config refuses unknown fields. The
//! caller passes the path, so no environment variable chooses it.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent};
use serde::Deserialize;

use super::key::{Key, parse_sequence, sequence_to_string};

/// A command a key can be bound to.
pub trait Command: Copy + Eq + fmt::Debug + 'static {
    /// Every command, in the order a help screen lists them.
    const ALL: &'static [Self];
    /// The name a keys file uses for the command.
    fn name(self) -> &'static str;
    /// One line for a help screen.
    fn description(self) -> &'static str;
}

/// The built-in key sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    /// The keys each tool documents: single letters, arrows, and the
    /// editing keys of the editor widget.
    #[default]
    Default,
    /// Emacs's keys, with ESC as a meta prefix.
    Emacs,
}

impl Preset {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "default" => Some(Self::Default),
            "emacs" => Some(Self::Emacs),
            _ => None,
        }
    }
}

/// One section of a keys file, and its preset bindings.
pub struct Section<A: 'static> {
    /// The section's name in the file.
    pub name: &'static str,
    /// The default preset: key sequence, command.
    pub default: &'static [(&'static str, A)],
    /// The Emacs preset.
    pub emacs: &'static [(&'static str, A)],
}

impl<A> Section<A> {
    fn preset(&self, preset: Preset) -> &'static [(&'static str, A)] {
        match preset {
            Preset::Default => self.default,
            Preset::Emacs => self.emacs,
        }
    }
}

/// The action name that removes a preset binding.
pub const UNBOUND: &str = "unbound";

/// Why a keys file was refused.
///
/// Its own type, not a variant of the store's `Error`: a keys file is
/// terminal configuration and touches no store, and every message names
/// the file, so a person can fix it from the message alone.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum KeymapError {
    #[error("{path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Yaml {
        path: PathBuf,
        source: yaml_serde::Error,
    },
    #[error("{path}: format {found} is newer than this build reads (1)")]
    Format { path: PathBuf, found: u64 },
    #[error("{path}: {field} must be {expected}")]
    Field {
        path: PathBuf,
        field: String,
        expected: &'static str,
    },
    #[error("{path}: unknown preset {name:?}; use default or emacs")]
    Preset { path: PathBuf, name: String },
    #[error("{path}: unknown section {section:?}; this tool reads {known}")]
    Section {
        path: PathBuf,
        section: String,
        known: String,
    },
    #[error("{path}: {section}: {reason}")]
    Key {
        path: PathBuf,
        section: String,
        reason: String,
    },
    #[error("{path}: {section}: {keys:?}: unknown command {action:?}")]
    Action {
        path: PathBuf,
        section: String,
        keys: String,
        action: String,
    },
    #[error(
        "{path}: {section}: {keys:?} starts {longer:?}, so {longer:?} could never be typed; \
         bind one of them"
    )]
    Prefix {
        path: PathBuf,
        section: String,
        keys: String,
        longer: String,
    },
}

/// A bound key sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding<A> {
    pub keys: Vec<Key>,
    pub action: A,
}

/// What one key event did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<A> {
    /// The keys so far complete a binding.
    Action(A),
    /// The keys so far start a binding; [`Keymap::pending`] names them.
    Pending,
    /// A single key that starts no binding. An editor inserts it when
    /// it is printable.
    Unbound(Key),
    /// A prefix followed by a key that completes nothing, named as
    /// Emacs names it: `C-x C-q is undefined`.
    Undefined(String),
    /// C-g cleared a pending prefix.
    Cancelled,
    /// The event was a release or a lone modifier.
    Ignored,
}

/// Bindings for one section, and the prefix typed so far.
#[derive(Debug, Clone)]
pub struct Keymap<A> {
    bindings: Vec<Binding<A>>,
    esc_is_meta: bool,
    pending: Vec<Key>,
    meta_next: bool,
}

const C_G: Key = Key {
    code: KeyCode::Char('g'),
    ctrl: true,
    meta: false,
    shift: false,
    super_key: false,
};

fn is_prefix(short: &[Key], long: &[Key]) -> bool {
    short.len() < long.len() && long.starts_with(short)
}

impl<A: Command> Keymap<A> {
    /// A section's preset bindings, with no file.
    ///
    /// # Panics
    ///
    /// When a preset table holds a malformed key or two bindings where
    /// one starts the other. The tables are constants, and each tool's
    /// tests build every preset, so this fails a test, not a user.
    #[must_use]
    pub fn preset(section: &Section<A>, preset: Preset) -> Self {
        let bindings = section
            .preset(preset)
            .iter()
            .map(|(keys, action)| Binding {
                keys: parse_sequence(keys)
                    .unwrap_or_else(|e| panic!("{} preset: {e}", section.name)),
                action: *action,
            })
            .collect::<Vec<_>>();
        for (i, a) in bindings.iter().enumerate() {
            for b in &bindings[i + 1..] {
                assert!(
                    a.keys != b.keys
                        && !is_prefix(&a.keys, &b.keys)
                        && !is_prefix(&b.keys, &a.keys),
                    "{} preset binds {} and {}",
                    section.name,
                    sequence_to_string(&a.keys),
                    sequence_to_string(&b.keys)
                );
            }
        }
        Self {
            bindings,
            esc_is_meta: preset == Preset::Emacs,
            pending: Vec::new(),
            meta_next: false,
        }
    }

    /// Turn one key event into what it asks for.
    pub fn feed(&mut self, event: &KeyEvent) -> Lookup<A> {
        let Some(mut key) = Key::from_event(event) else {
            return Lookup::Ignored;
        };
        if key == C_G && (self.meta_next || !self.pending.is_empty()) {
            self.clear();
            return Lookup::Cancelled;
        }
        if self.meta_next {
            self.meta_next = false;
            key = key.with_meta();
        }
        let mut keys = self.pending.clone();
        keys.push(key);
        if let Some(action) = self
            .bindings
            .iter()
            .find(|b| b.keys == keys)
            .map(|b| b.action)
        {
            self.clear();
            return Lookup::Action(action);
        }
        if self.bindings.iter().any(|b| is_prefix(&keys, &b.keys)) {
            self.pending = keys;
            return Lookup::Pending;
        }
        // ESC is a meta prefix in the Emacs preset unless a binding
        // claims it here, so ESC f is M-f and ESC ESC ESC is M-ESC ESC.
        if self.esc_is_meta && key == Key::plain(KeyCode::Esc) {
            self.meta_next = true;
            return Lookup::Pending;
        }
        let was_pending = !self.pending.is_empty();
        self.clear();
        if was_pending {
            Lookup::Undefined(format!("{} is undefined", sequence_to_string(&keys)))
        } else {
            Lookup::Unbound(key)
        }
    }

    /// Drop a pending prefix.
    pub fn clear(&mut self) {
        self.pending.clear();
        self.meta_next = false;
    }

    /// The pending prefix as Emacs echoes it, `C-x-`, or `None`.
    #[must_use]
    pub fn pending(&self) -> Option<String> {
        if self.pending.is_empty() && !self.meta_next {
            return None;
        }
        let mut text = sequence_to_string(&self.pending);
        if self.meta_next {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str("ESC");
        }
        text.push('-');
        Some(text)
    }

    /// Every binding, in the order they were laid down.
    #[must_use]
    pub fn bindings(&self) -> &[Binding<A>] {
        &self.bindings
    }

    /// The key sequences bound to a command, written as they parse.
    #[must_use]
    pub fn keys_for(&self, action: A) -> Vec<String> {
        self.bindings
            .iter()
            .filter(|b| b.action == action)
            .map(|b| sequence_to_string(&b.keys))
            .collect()
    }

    /// One row per bound command, in [`Command::ALL`] order: the
    /// command and its key sequences, separated by commas.
    #[must_use]
    pub fn help_rows(&self) -> Vec<(A, String)> {
        A::ALL
            .iter()
            .filter_map(|&a| {
                let keys = self.keys_for(a);
                (!keys.is_empty()).then(|| (a, keys.join(", ")))
            })
            .collect()
    }

    /// Lay a file section's entries over the preset.
    fn overlay(
        &mut self,
        path: &Path,
        section: &str,
        entries: &BTreeMap<String, String>,
    ) -> Result<(), KeymapError> {
        let mut user: Vec<(Vec<Key>, Option<A>, &str)> = Vec::new();
        for (keys_text, action_name) in entries {
            let keys = parse_sequence(keys_text).map_err(|e| KeymapError::Key {
                path: path.to_path_buf(),
                section: section.to_string(),
                reason: e.0,
            })?;
            let action = if action_name == UNBOUND {
                None
            } else {
                let found = A::ALL.iter().copied().find(|a| a.name() == action_name);
                Some(found.ok_or_else(|| KeymapError::Action {
                    path: path.to_path_buf(),
                    section: section.to_string(),
                    keys: keys_text.clone(),
                    action: action_name.clone(),
                })?)
            };
            user.push((keys, action, keys_text));
        }
        for (i, (a, _, a_text)) in user.iter().enumerate() {
            for (b, _, b_text) in &user[i + 1..] {
                let (short, long) = if is_prefix(a, b) {
                    (a_text, b_text)
                } else if is_prefix(b, a) {
                    (b_text, a_text)
                } else {
                    continue;
                };
                return Err(KeymapError::Prefix {
                    path: path.to_path_buf(),
                    section: section.to_string(),
                    keys: (*short).to_string(),
                    longer: (*long).to_string(),
                });
            }
        }
        // A user binding replaces every preset binding it collides with:
        // the same keys, a preset sequence it starts (rebinding the
        // prefix C-x drops the preset's C-x bindings, as in Emacs), and
        // a preset key it extends.
        for (keys, action, _) in user {
            self.bindings.retain(|b| {
                b.keys != keys && !is_prefix(&keys, &b.keys) && !is_prefix(&b.keys, &keys)
            });
            if let Some(action) = action {
                self.bindings.push(Binding { keys, action });
            }
        }
        Ok(())
    }
}

/// A user's keys file, read and checked for shape.
#[derive(Debug, Clone, Default)]
pub struct KeysFile {
    path: PathBuf,
    preset: Preset,
    sections: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Entry {
    Number(u64),
    Text(String),
    Section(BTreeMap<String, String>),
}

impl KeysFile {
    /// Read the file at `path`. An absent file is the default preset
    /// with no changes; any other problem is an error, never a quiet
    /// fall back to the defaults.
    pub fn load(path: &Path) -> Result<Self, KeymapError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text, path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                path: path.to_path_buf(),
                ..Self::default()
            }),
            Err(source) => Err(KeymapError::Read {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Parse a file's text. `path` names it in messages.
    pub fn parse(text: &str, path: &Path) -> Result<Self, KeymapError> {
        let path_buf = path.to_path_buf();
        let field = |field: &str, expected| KeymapError::Field {
            path: path_buf.clone(),
            field: field.to_string(),
            expected,
        };
        let mut file = Self {
            path: path_buf.clone(),
            ..Self::default()
        };
        if text.trim().is_empty() {
            return Ok(file);
        }
        let top: BTreeMap<String, Entry> =
            yaml_serde::from_str(text).map_err(|source| KeymapError::Yaml {
                path: path_buf.clone(),
                source,
            })?;
        for (name, entry) in top {
            match (name.as_str(), entry) {
                ("format", Entry::Number(n)) => {
                    if n > 1 {
                        return Err(KeymapError::Format {
                            path: path_buf,
                            found: n,
                        });
                    }
                }
                ("format", _) => return Err(field("format", "a number")),
                ("preset", Entry::Text(p)) => {
                    file.preset = Preset::parse(&p).ok_or_else(|| KeymapError::Preset {
                        path: path_buf.clone(),
                        name: p,
                    })?;
                }
                ("preset", _) => return Err(field("preset", "default or emacs")),
                (_, Entry::Section(entries)) => {
                    file.sections.insert(name, entries);
                }
                (other, _) => {
                    return Err(field(other, "a map of key sequences to commands"));
                }
            }
        }
        Ok(file)
    }

    /// The preset the file chose.
    #[must_use]
    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// Refuse a section this tool does not read, so a misspelt section
    /// is reported instead of ignored.
    pub fn check_sections(&self, known: &[&str]) -> Result<(), KeymapError> {
        match self.sections.keys().find(|s| !known.contains(&s.as_str())) {
            Some(section) => Err(KeymapError::Section {
                path: self.path.clone(),
                section: section.clone(),
                known: known.join(", "),
            }),
            None => Ok(()),
        }
    }

    /// The keymap for one section: the chosen preset, with the file's
    /// entries for that section laid over it.
    pub fn keymap<A: Command>(&self, section: &Section<A>) -> Result<Keymap<A>, KeymapError> {
        let mut keymap = Keymap::preset(section, self.preset);
        if let Some(entries) = self.sections.get(section.name) {
            keymap.overlay(&self.path, section.name, entries)?;
        }
        Ok(keymap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_crossterm::crossterm::event::KeyModifiers;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Act {
        Down,
        Save,
        Quit,
        Help,
    }

    impl Command for Act {
        const ALL: &'static [Self] = &[Self::Down, Self::Save, Self::Quit, Self::Help];
        fn name(self) -> &'static str {
            match self {
                Self::Down => "down",
                Self::Save => "save",
                Self::Quit => "quit",
                Self::Help => "help",
            }
        }
        fn description(self) -> &'static str {
            self.name()
        }
    }

    const SECTION: Section<Act> = Section {
        name: "list",
        default: &[
            ("j", Act::Down),
            ("<down>", Act::Down),
            ("q", Act::Quit),
            ("?", Act::Help),
        ],
        emacs: &[
            ("C-n", Act::Down),
            ("C-x C-s", Act::Save),
            ("C-x C-c", Act::Quit),
            ("C-h m", Act::Help),
        ],
    };

    fn key(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }
    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        key(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    fn esc() -> KeyEvent {
        key(KeyCode::Esc, KeyModifiers::NONE)
    }

    fn file(text: &str) -> Result<KeysFile, KeymapError> {
        KeysFile::parse(text, Path::new("keys.yml"))
    }

    #[test]
    fn an_absent_file_is_the_default_preset() {
        let dir = std::env::temp_dir().join(format!("mdstore-keys-absent-{}", std::process::id()));
        let f = KeysFile::load(&dir.join("keys.yml")).unwrap();
        assert_eq!(f.preset(), Preset::Default);
        let mut map = f.keymap(&SECTION).unwrap();
        assert_eq!(map.feed(&ch('j')), Lookup::Action(Act::Down));
    }

    #[test]
    fn an_unreadable_path_is_an_error_not_the_defaults() {
        let dir = std::env::temp_dir().join(format!("mdstore-keys-dir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let e = KeysFile::load(&dir).unwrap_err();
        assert!(matches!(e, KeymapError::Read { .. }), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_file_picks_the_preset() {
        let mut map = file("preset: emacs").unwrap().keymap(&SECTION).unwrap();
        assert_eq!(map.feed(&ctrl('n')), Lookup::Action(Act::Down));
        assert_eq!(
            map.feed(&ch('j')),
            Lookup::Unbound(Key::plain(KeyCode::Char('j')))
        );
    }

    #[test]
    fn a_sequence_completes_across_two_events() {
        let mut map = file("preset: emacs").unwrap().keymap(&SECTION).unwrap();
        assert_eq!(map.feed(&ctrl('x')), Lookup::Pending);
        assert_eq!(map.pending().as_deref(), Some("C-x-"));
        assert_eq!(map.feed(&ctrl('s')), Lookup::Action(Act::Save));
        assert_eq!(map.pending(), None);
    }

    #[test]
    fn c_g_clears_a_pending_prefix_in_every_preset() {
        let mut map = file("preset: emacs").unwrap().keymap(&SECTION).unwrap();
        map.feed(&ctrl('x'));
        assert_eq!(map.feed(&ctrl('g')), Lookup::Cancelled);
        assert_eq!(map.pending(), None);
        let mut map = file("list:\n  'g g': down\n")
            .unwrap()
            .keymap(&SECTION)
            .unwrap();
        map.feed(&ch('g'));
        assert_eq!(map.feed(&ctrl('g')), Lookup::Cancelled);
    }

    #[test]
    fn a_continuation_that_completes_nothing_is_undefined() {
        let mut map = file("preset: emacs").unwrap().keymap(&SECTION).unwrap();
        map.feed(&ctrl('x'));
        assert_eq!(
            map.feed(&ctrl('q')),
            Lookup::Undefined("C-x C-q is undefined".into())
        );
        assert_eq!(map.pending(), None);
    }

    #[test]
    fn esc_is_a_meta_prefix_in_the_emacs_preset_only() {
        let text = "preset: emacs\nlist:\n  M-n: down\n";
        let mut map = file(text).unwrap().keymap(&SECTION).unwrap();
        assert_eq!(map.feed(&esc()), Lookup::Pending);
        assert_eq!(map.pending().as_deref(), Some("ESC-"));
        assert_eq!(map.feed(&ch('n')), Lookup::Action(Act::Down));

        let mut map = file("list:\n  M-n: down\n")
            .unwrap()
            .keymap(&SECTION)
            .unwrap();
        assert_eq!(map.feed(&esc()), Lookup::Unbound(Key::plain(KeyCode::Esc)));
        assert_eq!(
            map.feed(&ch('n')),
            Lookup::Unbound(Key::plain(KeyCode::Char('n')))
        );
    }

    #[test]
    fn esc_esc_esc_reaches_a_binding_on_m_esc_esc() {
        let text = "preset: emacs\nlist:\n  M-ESC ESC: quit\n";
        let mut map = file(text).unwrap().keymap(&SECTION).unwrap();
        assert_eq!(map.feed(&esc()), Lookup::Pending);
        assert_eq!(map.feed(&esc()), Lookup::Pending);
        assert_eq!(map.feed(&esc()), Lookup::Action(Act::Quit));
    }

    #[test]
    fn a_user_binding_replaces_the_preset_binding_on_the_same_keys() {
        let mut map = file("list:\n  j: help\n")
            .unwrap()
            .keymap(&SECTION)
            .unwrap();
        assert_eq!(map.feed(&ch('j')), Lookup::Action(Act::Help));
        assert_eq!(map.keys_for(Act::Down), vec!["<down>"]);
    }

    #[test]
    fn unbound_removes_a_preset_binding() {
        let mut map = file("list:\n  q: unbound\n")
            .unwrap()
            .keymap(&SECTION)
            .unwrap();
        assert_eq!(
            map.feed(&ch('q')),
            Lookup::Unbound(Key::plain(KeyCode::Char('q')))
        );
        assert!(map.keys_for(Act::Quit).is_empty());
    }

    #[test]
    fn rebinding_a_preset_prefix_drops_the_bindings_under_it() {
        let text = "preset: emacs\nlist:\n  C-x: help\n";
        let mut map = file(text).unwrap().keymap(&SECTION).unwrap();
        assert_eq!(map.feed(&ctrl('x')), Lookup::Action(Act::Help));
        assert!(map.keys_for(Act::Save).is_empty());
        assert!(map.keys_for(Act::Quit).is_empty());
    }

    #[test]
    fn extending_a_preset_key_drops_the_preset_key() {
        let mut map = file("list:\n  q q: quit\n")
            .unwrap()
            .keymap(&SECTION)
            .unwrap();
        assert_eq!(map.feed(&ch('q')), Lookup::Pending);
        assert_eq!(map.feed(&ch('q')), Lookup::Action(Act::Quit));
    }

    #[test]
    fn two_user_bindings_where_one_starts_the_other_are_refused() {
        let e = file("list:\n  g: down\n  g g: help\n")
            .unwrap()
            .keymap(&SECTION)
            .unwrap_err();
        assert!(matches!(e, KeymapError::Prefix { .. }), "{e}");
        assert!(e.to_string().contains("keys.yml: list:"), "{e}");
    }

    #[test]
    fn errors_name_the_file_the_section_and_the_key() {
        let cases = [
            (
                "list:\n  ctrl+x: down\n",
                "keys.yml: list: ctrl+x: unknown key",
            ),
            (
                "list:\n  j: fly\n",
                "keys.yml: list: \"j\": unknown command \"fly\"",
            ),
            ("preset: vim\n", "unknown preset \"vim\""),
            ("format: 2\n", "format 2 is newer"),
            ("format: one\n", "format must be a number"),
            ("list: 3\n", "list must be a map"),
            ("list: [\n", "keys.yml:"),
        ];
        for (text, says) in cases {
            let e = file(text).and_then(|f| f.keymap(&SECTION).map(|_| ()));
            let msg = e.expect_err(text).to_string();
            assert!(msg.contains(says), "{text:?}: {msg}");
        }
    }

    #[test]
    fn an_unknown_section_is_refused() {
        let f = file("lsit:\n  j: down\n").unwrap();
        let e = f.check_sections(&["list", "editor"]).unwrap_err();
        assert_eq!(
            e.to_string(),
            "keys.yml: unknown section \"lsit\"; this tool reads list, editor"
        );
        file("list:\n  j: down\n")
            .unwrap()
            .check_sections(&["list"])
            .unwrap();
    }

    #[test]
    fn format_one_and_an_empty_file_are_accepted() {
        assert_eq!(file("format: 1\n").unwrap().preset(), Preset::Default);
        assert_eq!(file("").unwrap().preset(), Preset::Default);
    }

    #[test]
    fn help_rows_follow_the_command_order_and_list_every_key() {
        let map = Keymap::preset(&SECTION, Preset::Default);
        assert_eq!(
            map.help_rows(),
            vec![
                (Act::Down, "j, <down>".to_string()),
                (Act::Quit, "q".to_string()),
                (Act::Help, "?".to_string()),
            ]
        );
    }

    #[test]
    fn a_release_is_ignored() {
        let mut map = Keymap::preset(&SECTION, Preset::Default);
        let mut release = ch('j');
        release.kind = ratatui_crossterm::crossterm::event::KeyEventKind::Release;
        assert_eq!(map.feed(&release), Lookup::Ignored);
    }
}
