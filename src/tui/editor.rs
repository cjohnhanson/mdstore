//! The markdown editor the mdstore tools open on a document body.
//!
//! [`Editor`] wraps ratatui-textarea and drives it only through its
//! public editing methods, from a [`Keymap`] of [`EditAction`]s, so the
//! configured keys are the only keys. It owns what the widget lacks for
//! Emacs editing: a kill ring, a mark that survives motion, `C-x`
//! prefixes, incremental search, and an echo line.
//!
//! The editor never touches a store. [`Editor::handle`] returns
//! [`Outcome::Save`] (keep editing) or [`Outcome::Finish`] (close) with
//! the whole text, and the tool writes it through its own core
//! operation, so frontmatter, provenance, and refusals stay in the tool.
//! Before writing, the tool compares the document on disk with
//! [`Editor::original`] and refuses when someone else changed it. After
//! a save that keeps the editor open, the tool calls [`Editor::saved`].
//!
//! A compound command (yank-pop, transpose, a case change) is several
//! widget edits, and the widget has no way to group its undo history, so
//! undo reverses such a command in steps.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::text::Line;
use ratatui_core::widgets::Widget;
use ratatui_crossterm::crossterm::event::KeyEvent;
use ratatui_textarea::{CursorMove, Scrolling, TextArea};
use ratatui_widgets::block::Block;

use super::keymap::{Command, Keymap, Lookup, Section};
use super::kill_ring::{Direction, KillRing};

/// What the editor's keys can do. The names are Emacs's command names,
/// except `backward-kill-line`, which Emacs has only as `C-0 C-k`, and
/// `with-editor-finish`, from the with-editor package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    ForwardChar,
    BackwardChar,
    NextLine,
    PreviousLine,
    ForwardWord,
    BackwardWord,
    BeginningOfLine,
    EndOfLine,
    BeginningOfBuffer,
    EndOfBuffer,
    ScrollUp,
    ScrollDown,
    ForwardParagraph,
    BackwardParagraph,
    Newline,
    OpenLine,
    DeleteChar,
    DeleteBackwardChar,
    KillWord,
    BackwardKillWord,
    KillLine,
    BackwardKillLine,
    TransposeChars,
    UpcaseWord,
    DowncaseWord,
    CapitalizeWord,
    Indent,
    Undo,
    Redo,
    KillRegion,
    CopyRegion,
    Yank,
    YankPop,
    SetMark,
    ExchangePointAndMark,
    MarkWholeBuffer,
    IsearchForward,
    IsearchBackward,
    KeyboardQuit,
    Save,
    Finish,
    Cancel,
}

impl Command for EditAction {
    const ALL: &'static [Self] = &[
        Self::Save,
        Self::Finish,
        Self::Cancel,
        Self::KeyboardQuit,
        Self::ForwardChar,
        Self::BackwardChar,
        Self::NextLine,
        Self::PreviousLine,
        Self::ForwardWord,
        Self::BackwardWord,
        Self::BeginningOfLine,
        Self::EndOfLine,
        Self::BeginningOfBuffer,
        Self::EndOfBuffer,
        Self::ScrollUp,
        Self::ScrollDown,
        Self::ForwardParagraph,
        Self::BackwardParagraph,
        Self::Newline,
        Self::OpenLine,
        Self::DeleteChar,
        Self::DeleteBackwardChar,
        Self::KillWord,
        Self::BackwardKillWord,
        Self::KillLine,
        Self::BackwardKillLine,
        Self::TransposeChars,
        Self::UpcaseWord,
        Self::DowncaseWord,
        Self::CapitalizeWord,
        Self::Indent,
        Self::Undo,
        Self::Redo,
        Self::SetMark,
        Self::ExchangePointAndMark,
        Self::MarkWholeBuffer,
        Self::KillRegion,
        Self::CopyRegion,
        Self::Yank,
        Self::YankPop,
        Self::IsearchForward,
        Self::IsearchBackward,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::ForwardChar => "forward-char",
            Self::BackwardChar => "backward-char",
            Self::NextLine => "next-line",
            Self::PreviousLine => "previous-line",
            Self::ForwardWord => "forward-word",
            Self::BackwardWord => "backward-word",
            Self::BeginningOfLine => "move-beginning-of-line",
            Self::EndOfLine => "move-end-of-line",
            Self::BeginningOfBuffer => "beginning-of-buffer",
            Self::EndOfBuffer => "end-of-buffer",
            Self::ScrollUp => "scroll-up-command",
            Self::ScrollDown => "scroll-down-command",
            Self::ForwardParagraph => "forward-paragraph",
            Self::BackwardParagraph => "backward-paragraph",
            Self::Newline => "newline",
            Self::OpenLine => "open-line",
            Self::DeleteChar => "delete-char",
            Self::DeleteBackwardChar => "delete-backward-char",
            Self::KillWord => "kill-word",
            Self::BackwardKillWord => "backward-kill-word",
            Self::KillLine => "kill-line",
            Self::BackwardKillLine => "backward-kill-line",
            Self::TransposeChars => "transpose-chars",
            Self::UpcaseWord => "upcase-word",
            Self::DowncaseWord => "downcase-word",
            Self::CapitalizeWord => "capitalize-word",
            Self::Indent => "indent-for-tab-command",
            Self::Undo => "undo",
            Self::Redo => "undo-redo",
            Self::KillRegion => "kill-region",
            Self::CopyRegion => "kill-ring-save",
            Self::Yank => "yank",
            Self::YankPop => "yank-pop",
            Self::SetMark => "set-mark-command",
            Self::ExchangePointAndMark => "exchange-point-and-mark",
            Self::MarkWholeBuffer => "mark-whole-buffer",
            Self::IsearchForward => "isearch-forward",
            Self::IsearchBackward => "isearch-backward",
            Self::KeyboardQuit => "keyboard-quit",
            Self::Save => "save-buffer",
            Self::Finish => "with-editor-finish",
            Self::Cancel => "kill-buffer",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::ForwardChar => "forward a character",
            Self::BackwardChar => "back a character",
            Self::NextLine => "down a line",
            Self::PreviousLine => "up a line",
            Self::ForwardWord => "forward a word",
            Self::BackwardWord => "back a word",
            Self::BeginningOfLine => "start of the line",
            Self::EndOfLine => "end of the line",
            Self::BeginningOfBuffer => "start of the text",
            Self::EndOfBuffer => "end of the text",
            Self::ScrollUp => "a page down",
            Self::ScrollDown => "a page up",
            Self::ForwardParagraph => "forward a paragraph",
            Self::BackwardParagraph => "back a paragraph",
            Self::Newline => "new line",
            Self::OpenLine => "new line after the cursor",
            Self::DeleteChar => "delete the next character, or the region",
            Self::DeleteBackwardChar => "delete the previous character, or the region",
            Self::KillWord => "kill to the end of the word",
            Self::BackwardKillWord => "kill to the start of the word",
            Self::KillLine => "kill to the end of the line",
            Self::BackwardKillLine => "kill to the start of the line",
            Self::TransposeChars => "swap the characters around the cursor",
            Self::UpcaseWord => "upper-case the next word",
            Self::DowncaseWord => "lower-case the next word",
            Self::CapitalizeWord => "capitalise the next word",
            Self::Indent => "indent with spaces to the next tab stop",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::KillRegion => "kill the region",
            Self::CopyRegion => "copy the region to the kill ring",
            Self::Yank => "insert the last kill",
            Self::YankPop => "replace the insert with an older kill",
            Self::SetMark => "set the mark and start a region",
            Self::ExchangePointAndMark => "swap the cursor and the mark",
            Self::MarkWholeBuffer => "select all",
            Self::IsearchForward => "search forward as you type",
            Self::IsearchBackward => "search backward as you type",
            Self::KeyboardQuit => "cancel the region, a prefix, or a search",
            Self::Save => "save and keep editing",
            Self::Finish => "save and close",
            Self::Cancel => "close without saving",
        }
    }
}

use EditAction as E;

/// The editor's section in a keys file, and its two presets.
///
/// The default preset is the key table in ratatui-textarea's README,
/// with `C-s` to save, `ESC` to close, `C-SPC` to start a region, and
/// `TAB` to indent. The Emacs preset is Emacs's own keys.
pub const SECTION: Section<EditAction> = Section {
    name: "editor",
    default: &[
        ("C-s", E::Save),
        ("ESC", E::Cancel),
        ("C-g", E::KeyboardQuit),
        ("C-h", E::DeleteBackwardChar),
        ("DEL", E::DeleteBackwardChar),
        ("C-d", E::DeleteChar),
        ("<delete>", E::DeleteChar),
        ("RET", E::Newline),
        ("C-k", E::KillLine),
        ("C-j", E::BackwardKillLine),
        ("C-w", E::BackwardKillWord),
        ("M-h", E::BackwardKillWord),
        ("M-DEL", E::BackwardKillWord),
        ("M-d", E::KillWord),
        ("M-<delete>", E::KillWord),
        ("C-u", E::Undo),
        ("C-r", E::Redo),
        ("C-SPC", E::SetMark),
        ("C-c", E::CopyRegion),
        ("C-x", E::KillRegion),
        ("C-y", E::Yank),
        ("C-f", E::ForwardChar),
        ("<right>", E::ForwardChar),
        ("C-b", E::BackwardChar),
        ("<left>", E::BackwardChar),
        ("C-p", E::PreviousLine),
        ("<up>", E::PreviousLine),
        ("C-n", E::NextLine),
        ("<down>", E::NextLine),
        ("M-f", E::ForwardWord),
        ("C-<right>", E::ForwardWord),
        ("M-b", E::BackwardWord),
        ("C-<left>", E::BackwardWord),
        ("M-]", E::BackwardParagraph),
        ("M-p", E::BackwardParagraph),
        ("C-<up>", E::BackwardParagraph),
        ("M-[", E::ForwardParagraph),
        ("M-n", E::ForwardParagraph),
        ("C-<down>", E::ForwardParagraph),
        ("C-e", E::EndOfLine),
        ("<end>", E::EndOfLine),
        ("C-M-f", E::EndOfLine),
        ("C-M-<right>", E::EndOfLine),
        ("C-a", E::BeginningOfLine),
        ("<home>", E::BeginningOfLine),
        ("C-M-b", E::BeginningOfLine),
        ("C-M-<left>", E::BeginningOfLine),
        ("M-<", E::BeginningOfBuffer),
        ("C-M-p", E::BeginningOfBuffer),
        ("C-M-<up>", E::BeginningOfBuffer),
        ("M->", E::EndOfBuffer),
        ("C-M-n", E::EndOfBuffer),
        ("C-M-<down>", E::EndOfBuffer),
        ("C-v", E::ScrollUp),
        ("<next>", E::ScrollUp),
        ("M-v", E::ScrollDown),
        ("<prior>", E::ScrollDown),
        ("TAB", E::Indent),
    ],
    emacs: &[
        ("C-x C-s", E::Save),
        ("C-c C-c", E::Finish),
        ("C-x #", E::Finish),
        ("C-x C-c", E::Cancel),
        ("C-x k", E::Cancel),
        ("C-c C-k", E::Cancel),
        ("C-g", E::KeyboardQuit),
        ("M-ESC ESC", E::KeyboardQuit),
        ("C-f", E::ForwardChar),
        ("<right>", E::ForwardChar),
        ("C-b", E::BackwardChar),
        ("<left>", E::BackwardChar),
        ("C-n", E::NextLine),
        ("<down>", E::NextLine),
        ("C-p", E::PreviousLine),
        ("<up>", E::PreviousLine),
        ("M-f", E::ForwardWord),
        ("C-<right>", E::ForwardWord),
        ("M-b", E::BackwardWord),
        ("C-<left>", E::BackwardWord),
        ("C-a", E::BeginningOfLine),
        ("<home>", E::BeginningOfLine),
        ("C-e", E::EndOfLine),
        ("<end>", E::EndOfLine),
        ("M-<", E::BeginningOfBuffer),
        ("M->", E::EndOfBuffer),
        ("C-v", E::ScrollUp),
        ("<next>", E::ScrollUp),
        ("M-v", E::ScrollDown),
        ("<prior>", E::ScrollDown),
        ("M-}", E::ForwardParagraph),
        ("C-<down>", E::ForwardParagraph),
        ("M-{", E::BackwardParagraph),
        ("C-<up>", E::BackwardParagraph),
        ("RET", E::Newline),
        ("C-o", E::OpenLine),
        ("C-d", E::DeleteChar),
        ("<delete>", E::DeleteChar),
        ("DEL", E::DeleteBackwardChar),
        ("M-d", E::KillWord),
        ("M-DEL", E::BackwardKillWord),
        ("C-k", E::KillLine),
        ("C-t", E::TransposeChars),
        ("M-u", E::UpcaseWord),
        ("M-l", E::DowncaseWord),
        ("M-c", E::CapitalizeWord),
        ("TAB", E::Indent),
        ("C-/", E::Undo),
        ("C-x u", E::Undo),
        // Emacs's C-M-_; the terminal sends it as C-M-/.
        ("C-M-/", E::Redo),
        ("C-w", E::KillRegion),
        ("M-w", E::CopyRegion),
        ("C-y", E::Yank),
        ("M-y", E::YankPop),
        ("C-SPC", E::SetMark),
        ("C-x C-x", E::ExchangePointAndMark),
        ("C-x h", E::MarkWholeBuffer),
        ("C-s", E::IsearchForward),
        ("C-r", E::IsearchBackward),
    ],
};

/// What a key press left the tool to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Keep the editor open.
    Continue,
    /// Write this text through the tool's core and keep the editor open,
    /// as Emacs's save-buffer does. Then call [`Editor::saved`], or
    /// [`Editor::set_message`] with the refusal.
    Save(String),
    /// Write this text and close the editor, as with-editor's finish
    /// does. On a refusal, keep the editor open with
    /// [`Editor::set_message`].
    Finish(String),
    /// Close the editor and write nothing.
    Cancel,
}

type Pos = (usize, usize);

/// What the previous command was, for the commands that care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Last {
    Other,
    Kill,
    Yank { start: Pos, end: Pos },
    SetMark,
}

#[derive(Debug, Clone)]
struct Search {
    query: String,
    origin: Pos,
    match_start: Pos,
    forward: bool,
    failing: bool,
}

#[derive(Debug, Clone)]
enum Mode {
    Edit,
    Search(Search),
    ConfirmDiscard,
}

/// A text editor for one document body, or one line of a prompt.
pub struct Editor {
    textarea: TextArea<'static>,
    keymap: Keymap<EditAction>,
    ring: KillRing,
    mark: Option<Pos>,
    last: Last,
    mode: Mode,
    message: Option<String>,
    last_search: String,
    original: String,
    single_line: bool,
}

fn lines_of(text: &str) -> Vec<String> {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

/// A position as a character offset into the lines joined by `\n`.
fn offset(lines: &[String], (row, col): Pos) -> usize {
    let row = row.min(lines.len().saturating_sub(1));
    let width = lines.get(row).map_or(0, |l| l.chars().count());
    lines[..row]
        .iter()
        .map(|l| l.chars().count() + 1)
        .sum::<usize>()
        + col.min(width)
}

fn joined(lines: &[String]) -> String {
    lines.join("\n")
}

/// A word character, as Emacs's text mode counts one.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The position at a character offset into the lines joined by `\n`.
fn position(lines: &[String], mut at: usize) -> Pos {
    for (row, line) in lines.iter().enumerate() {
        let len = line.chars().count();
        if at <= len {
            return (row, at);
        }
        at -= len + 1;
    }
    let last = lines.len().saturating_sub(1);
    (last, lines.get(last).map_or(0, |l| l.chars().count()))
}

/// The span one command changed: where it starts, how many characters
/// it removed, and how many it inserted. Found by comparing the text
/// before and after, so it is where the edit happened whatever the
/// command did with point (undo and redo move point to the edit).
///
/// Inside a run of equal characters (indentation, blank lines) several
/// spans describe the same change, and the comparison finds the
/// rightmost. An insertion or deletion slides left through the run, no
/// further than `anchor`, the cursor offset nearest the edit, so the
/// span sits where the command made it.
fn edit_span(before: &[char], after: &[char], anchor: usize) -> (usize, usize, usize) {
    let prefix = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let room = before.len().min(after.len()) - prefix;
    let suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    let mut start = prefix;
    let removed = before.len() - prefix - suffix;
    let inserted = after.len() - prefix - suffix;
    // Only a pure insertion or deletion has equal alternatives.
    let changed = match (removed, inserted) {
        (0, n) if n > 0 => Some((after, n)),
        (n, 0) if n > 0 => Some((before, n)),
        _ => None,
    };
    if let Some((text, len)) = changed {
        while start > anchor && text[start - 1] == text[start - 1 + len] {
            start -= 1;
        }
    }
    (start, removed, inserted)
}

/// Where a mark goes across an edit, as an Emacs marker moves: an edit
/// wholly before it shifts it by the change in length, an edit at or
/// after it leaves it, and an edit around it pulls it to the edit's
/// start.
///
/// A replacement of the same length, such as a case change or a
/// transpose, edits in place and leaves a mark inside it, as Emacs's
/// in-place commands do.
fn shift_mark(mark: usize, (start, removed, inserted): (usize, usize, usize)) -> usize {
    if mark <= start || removed == inserted {
        mark
    } else if mark >= start + removed {
        mark - removed + inserted
    } else {
        start
    }
}

/// The widget's jump takes 16-bit coordinates, so a position past line
/// or column 65,535 is clamped there.
fn jump_to((row, col): Pos) -> CursorMove {
    let row = u16::try_from(row).unwrap_or(u16::MAX);
    let col = u16::try_from(col).unwrap_or(u16::MAX);
    CursorMove::Jump(row, col)
}

impl Editor {
    /// An editor on a document body.
    #[must_use]
    pub fn new(text: &str, keymap: Keymap<EditAction>) -> Self {
        Self::build(text, keymap, false)
    }

    /// A one-line editor for a prompt, which acts as Emacs's minibuffer:
    /// `RET` and save finish ([`Outcome::Finish`]), keyboard-quit and
    /// kill-buffer cancel without asking, search does nothing, and a
    /// yank of several lines inserts them joined by spaces.
    ///
    /// A prompt draws only its text. A tool that shows a pending prefix
    /// or a message draws [`Editor::echo`] itself, beside the prompt.
    #[must_use]
    pub fn single_line(text: &str, keymap: Keymap<EditAction>) -> Self {
        let first = text.lines().next().unwrap_or("");
        Self::build(first, keymap, true)
    }

    fn build(text: &str, keymap: Keymap<EditAction>, single_line: bool) -> Self {
        let mut textarea = TextArea::new(lines_of(text));
        textarea.set_cursor_line_style(ratatui_core::style::Style::default());
        Self {
            textarea,
            keymap,
            ring: KillRing::default(),
            mark: None,
            last: Last::Other,
            mode: Mode::Edit,
            message: None,
            last_search: String::new(),
            original: text.to_string(),
            single_line,
        }
    }

    /// The text as it was when the editor opened.
    #[must_use]
    pub fn original(&self) -> &str {
        &self.original
    }

    /// The text now. A body ends with exactly one newline unless it is
    /// empty, so blank lines at the end are dropped; a prompt has none.
    /// Lines end with `\n`, so a body opened with `\r\n` saves with `\n`.
    #[must_use]
    pub fn text(&self) -> String {
        let text = joined(self.textarea.lines());
        if self.single_line {
            return text;
        }
        let trimmed = text.trim_end_matches('\n');
        if trimmed.is_empty() {
            String::new()
        } else {
            format!("{trimmed}\n")
        }
    }

    /// Whether the text differs from what the editor opened with.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        let opened = Self::build(&self.original, self.keymap.clone(), self.single_line);
        self.text() != opened.text()
    }

    /// Record that the tool wrote the text: it becomes the text the
    /// editor compares against, so closing asks nothing until the next
    /// change.
    pub fn saved(&mut self) {
        self.original = self.text();
        self.message = Some("Saved".into());
    }

    /// Show a message on the echo line, such as a refused save.
    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    /// The echo line: a pending prefix, the search, a question, or the
    /// last message.
    #[must_use]
    pub fn echo(&self) -> Option<String> {
        match &self.mode {
            Mode::Search(s) => {
                let failing = if s.failing { "Failing " } else { "" };
                let dir = if s.forward { "" } else { " backward" };
                Some(format!("{failing}I-search{dir}: {}", s.query))
            }
            Mode::ConfirmDiscard => Some(match &self.message {
                Some(m) => format!("{m}  Discard changes? (y or n)"),
                None => "Discard changes? (y or n)".to_string(),
            }),
            Mode::Edit => self.keymap.pending().or_else(|| self.message.clone()),
        }
    }

    /// The keymap, for a help screen.
    #[must_use]
    pub fn keymap(&self) -> &Keymap<EditAction> {
        &self.keymap
    }

    /// Draw a border and title around the text.
    pub fn set_block(&mut self, block: Block<'static>) {
        self.textarea.set_block(block);
    }

    /// Text shown while the editor is empty.
    pub fn set_placeholder_text(&mut self, text: impl Into<String>) {
        self.textarea.set_placeholder_text(text);
    }

    /// The cursor as (line, column), both from zero.
    #[must_use]
    pub fn cursor(&self) -> Pos {
        let c = self.textarea.cursor();
        (c.0, c.1)
    }

    /// Handle one key press.
    pub fn handle(&mut self, event: &KeyEvent) -> Outcome {
        match self.mode {
            Mode::ConfirmDiscard => self.confirm(event),
            Mode::Search(_) => self.search_key(event),
            Mode::Edit => {
                let lookup = self.keymap.feed(event);
                if lookup != Lookup::Ignored && lookup != Lookup::Pending {
                    self.message = None;
                }
                self.dispatch(lookup)
            }
        }
    }

    /// Run one command, and keep the mark on the same text across
    /// whatever it changed, unless the command set the mark itself.
    fn dispatch(&mut self, lookup: Lookup<EditAction>) -> Outcome {
        // Each key with a mark set costs two copies of the text; a
        // document body is small enough for that.
        let lines = self.textarea.lines();
        let before: Vec<char> = joined(lines).chars().collect();
        let cursor_before = offset(lines, self.cursor());
        let mark_before = self.mark;
        let mark_offset = mark_before.map(|m| offset(lines, m));
        let outcome = self.command(lookup);
        if let (Some(mark), true) = (mark_offset, self.mark == mark_before) {
            let lines = self.textarea.lines();
            let after: Vec<char> = joined(lines).chars().collect();
            if after != before {
                let anchor = cursor_before.min(offset(lines, self.cursor()));
                let span = edit_span(&before, &after, anchor);
                self.mark = Some(position(lines, shift_mark(mark, span)));
            }
        }
        outcome
    }

    fn command(&mut self, lookup: Lookup<EditAction>) -> Outcome {
        match lookup {
            Lookup::Ignored | Lookup::Pending => Outcome::Continue,
            Lookup::Cancelled => {
                self.message = Some("Quit".into());
                Outcome::Continue
            }
            Lookup::Undefined(text) => {
                self.message = Some(text);
                Outcome::Continue
            }
            Lookup::Unbound(key) => {
                if let Some(c) = key.printable() {
                    self.textarea.cancel_selection();
                    self.textarea.insert_char(c);
                    self.last = Last::Other;
                } else {
                    self.message = Some(format!("{key} is undefined"));
                }
                Outcome::Continue
            }
            Lookup::Action(action) => self.run(action),
        }
    }

    fn confirm(&mut self, event: &KeyEvent) -> Outcome {
        let Some(key) = super::key::Key::from_event(event) else {
            return Outcome::Continue;
        };
        let quit = self
            .keymap
            .bindings()
            .iter()
            .any(|b| b.action == E::KeyboardQuit && b.keys == [key]);
        match key.printable() {
            Some('y') => Outcome::Cancel,
            Some('n') => {
                self.mode = Mode::Edit;
                self.message = None;
                Outcome::Continue
            }
            _ if quit || key == super::key::C_G => {
                self.mode = Mode::Edit;
                self.message = Some("Quit".into());
                Outcome::Continue
            }
            _ => {
                self.message = Some("Please answer y or n.".into());
                Outcome::Continue
            }
        }
    }

    fn jump(&mut self, pos: Pos) {
        self.textarea.move_cursor(jump_to(pos));
    }

    /// Where Emacs's forward-word or backward-word lands: past the
    /// characters that are not part of a word, across lines, then past
    /// the word.
    fn word_target(&self, direction: Direction) -> Pos {
        let lines = self.textarea.lines();
        let (mut row, mut col) = self.cursor();
        let chars = |row: usize| -> Vec<char> { lines[row].chars().collect() };
        let mut line = chars(row);
        match direction {
            Direction::Forward => {
                loop {
                    while col < line.len() && !is_word(line[col]) {
                        col += 1;
                    }
                    if col < line.len() || row + 1 >= lines.len() {
                        break;
                    }
                    row += 1;
                    col = 0;
                    line = chars(row);
                }
                while col < line.len() && is_word(line[col]) {
                    col += 1;
                }
            }
            Direction::Backward => {
                loop {
                    while col > 0 && !is_word(line[col - 1]) {
                        col -= 1;
                    }
                    if col > 0 || row == 0 {
                        break;
                    }
                    row -= 1;
                    line = chars(row);
                    col = line.len();
                }
                while col > 0 && is_word(line[col - 1]) {
                    col -= 1;
                }
            }
        }
        (row, col)
    }

    fn region_active(&self) -> bool {
        self.textarea.selection_range().is_some()
    }

    /// Run a deletion, and put what it removed on the kill ring.
    fn kill(&mut self, direction: Direction, delete: impl FnOnce(&mut TextArea<'static>) -> bool) {
        let before: Vec<char> = joined(self.textarea.lines()).chars().collect();
        if !delete(&mut self.textarea) {
            // A kill that removed nothing still breaks a run of kills.
            self.last = Last::Other;
            return;
        }
        let after_len = joined(self.textarea.lines()).chars().count();
        let start = offset(self.textarea.lines(), self.cursor());
        let removed = before.len().saturating_sub(after_len);
        let killed: String = before.iter().skip(start).take(removed).collect();
        self.ring.kill(killed, direction, self.last == Last::Kill);
        self.last = Last::Kill;
    }

    fn run(&mut self, action: EditAction) -> Outcome {
        let previous = self.last;
        self.last = Last::Other;
        let ta = &mut self.textarea;
        match action {
            E::ForwardChar => ta.move_cursor(CursorMove::Forward),
            E::BackwardChar => ta.move_cursor(CursorMove::Back),
            E::NextLine => ta.move_cursor(CursorMove::Down),
            E::PreviousLine => ta.move_cursor(CursorMove::Up),
            E::ForwardWord => {
                let target = self.word_target(Direction::Forward);
                self.jump(target);
            }
            E::BackwardWord => {
                let target = self.word_target(Direction::Backward);
                self.jump(target);
            }
            E::BeginningOfLine => ta.move_cursor(CursorMove::Head),
            E::EndOfLine => ta.move_cursor(CursorMove::End),
            E::BeginningOfBuffer => {
                ta.move_cursor(CursorMove::Top);
                ta.move_cursor(CursorMove::Head);
            }
            E::EndOfBuffer => {
                ta.move_cursor(CursorMove::Bottom);
                ta.move_cursor(CursorMove::End);
            }
            E::ScrollUp => ta.scroll(Scrolling::PageDown),
            E::ScrollDown => ta.scroll(Scrolling::PageUp),
            E::ForwardParagraph => ta.move_cursor(CursorMove::ParagraphForward),
            E::BackwardParagraph => ta.move_cursor(CursorMove::ParagraphBack),
            E::Newline => {
                if self.single_line {
                    return Outcome::Finish(self.text());
                }
                ta.cancel_selection();
                ta.insert_newline();
            }
            E::OpenLine => {
                if !self.single_line {
                    ta.cancel_selection();
                    ta.insert_newline();
                    ta.move_cursor(CursorMove::Back);
                }
            }
            // With a region active, the widget deletes the region, as
            // Emacs's delete-active-region does.
            E::DeleteChar => {
                ta.delete_next_char();
            }
            E::DeleteBackwardChar => {
                ta.delete_char();
            }
            E::KillWord | E::BackwardKillWord => {
                ta.cancel_selection();
                self.last = previous;
                let direction = if action == E::KillWord {
                    Direction::Forward
                } else {
                    Direction::Backward
                };
                let target = self.word_target(direction);
                self.kill(direction, |ta| {
                    ta.start_selection();
                    ta.move_cursor(jump_to(target));
                    ta.cut()
                });
            }
            E::KillLine => {
                ta.cancel_selection();
                self.last = previous;
                self.kill(Direction::Forward, TextArea::delete_line_by_end);
            }
            E::BackwardKillLine => {
                ta.cancel_selection();
                self.last = previous;
                self.kill(Direction::Backward, TextArea::delete_line_by_head);
            }
            E::TransposeChars => self.transpose(),
            E::UpcaseWord => self.case_word(|w| w.to_uppercase()),
            E::DowncaseWord => self.case_word(|w| w.to_lowercase()),
            E::CapitalizeWord => self.case_word(|w| {
                let mut chars = w.chars();
                chars.next().map_or_else(String::new, |first| {
                    first
                        .to_uppercase()
                        .chain(chars.flat_map(char::to_lowercase))
                        .collect()
                })
            }),
            E::Indent => {
                ta.cancel_selection();
                ta.insert_tab();
            }
            E::Undo => {
                ta.cancel_selection();
                ta.undo();
            }
            E::Redo => {
                ta.cancel_selection();
                ta.redo();
            }
            E::KillRegion => {
                if !self.select_region() {
                    return Outcome::Continue;
                }
                // Point before the mark kills backward, so a run of kills
                // prepends, as in Emacs.
                let point_first = self
                    .textarea
                    .selection_range()
                    .is_some_and(|(start, _)| start == self.cursor());
                let direction = if point_first {
                    Direction::Backward
                } else {
                    Direction::Forward
                };
                self.last = previous;
                self.kill(direction, TextArea::cut);
            }
            E::CopyRegion => {
                if !self.select_region() {
                    return Outcome::Continue;
                }
                self.textarea.copy();
                let text = self.textarea.yank_text();
                self.ring.kill(text, Direction::Forward, false);
                self.textarea.cancel_selection();
            }
            E::Yank => self.yank(),
            E::YankPop => self.yank_pop(previous),
            E::SetMark => {
                ta.cancel_selection();
                // A second C-SPC straight after the first ends the region
                // and keeps the mark, as in Emacs.
                if previous == Last::SetMark {
                    self.message = Some("Mark deactivated".into());
                } else {
                    ta.start_selection();
                    self.mark = Some(self.cursor());
                    self.message = Some("Mark set".into());
                    self.last = Last::SetMark;
                }
            }
            E::ExchangePointAndMark => match self.mark {
                Some(mark) => {
                    let point = self.cursor();
                    self.textarea.cancel_selection();
                    self.jump(point);
                    self.textarea.start_selection();
                    self.jump(mark);
                    self.mark = Some(point);
                }
                None => self.message = Some("No mark set in this buffer".into()),
            },
            E::MarkWholeBuffer => {
                ta.cancel_selection();
                ta.move_cursor(CursorMove::Bottom);
                ta.move_cursor(CursorMove::End);
                ta.start_selection();
                self.mark = Some(self.cursor());
                self.textarea.move_cursor(CursorMove::Top);
                self.textarea.move_cursor(CursorMove::Head);
            }
            // A prompt has one short line and no echo row to search on.
            E::IsearchForward | E::IsearchBackward if self.single_line => {}
            E::IsearchForward | E::IsearchBackward => {
                ta.cancel_selection();
                let here = self.cursor();
                self.mode = Mode::Search(Search {
                    query: String::new(),
                    origin: here,
                    match_start: here,
                    forward: action == E::IsearchForward,
                    failing: false,
                });
            }
            // In a prompt, C-g closes it, as in Emacs's minibuffer.
            E::KeyboardQuit if self.single_line => return Outcome::Cancel,
            E::KeyboardQuit => {
                ta.cancel_selection();
                self.message = Some("Quit".into());
            }
            // A prompt has nothing to keep editing after a save, so a
            // save submits it.
            E::Save if self.single_line => return Outcome::Finish(self.text()),
            E::Save => return Outcome::Save(self.text()),
            E::Finish => return Outcome::Finish(self.text()),
            E::Cancel => {
                // A prompt draws no echo line to ask on, and closes
                // without asking, as Emacs's minibuffer does on C-g.
                if self.is_dirty() && !self.single_line {
                    self.mode = Mode::ConfirmDiscard;
                } else {
                    return Outcome::Cancel;
                }
            }
        }
        Outcome::Continue
    }

    /// Make the region the selection: the active one, or point to mark.
    fn select_region(&mut self) -> bool {
        if self.region_active() {
            return true;
        }
        let Some(mark) = self.mark else {
            self.message = Some("The mark is not set now, so there is no region".into());
            return false;
        };
        let point = self.cursor();
        self.jump(mark);
        self.textarea.start_selection();
        self.jump(point);
        true
    }

    fn insert_yank(&mut self, text: &str) {
        let text = if self.single_line {
            text.replace('\n', " ")
        } else {
            text.to_string()
        };
        self.textarea.set_yank_text(text);
        self.textarea.paste();
    }

    fn yank(&mut self) {
        let Some(text) = self.ring.yank().map(str::to_string) else {
            self.message = Some("Kill ring is empty".into());
            return;
        };
        self.textarea.cancel_selection();
        let start = self.cursor();
        self.insert_yank(&text);
        self.mark = Some(start);
        self.last = Last::Yank {
            start,
            end: self.cursor(),
        };
    }

    fn yank_pop(&mut self, previous: Last) {
        let Last::Yank { start, end } = previous else {
            self.message = Some("Previous command was not a yank".into());
            return;
        };
        let Some(text) = self.ring.pop().map(str::to_string) else {
            return;
        };
        self.textarea.cancel_selection();
        self.jump(start);
        self.textarea.start_selection();
        self.jump(end);
        self.textarea.cut();
        self.insert_yank(&text);
        self.last = Last::Yank {
            start,
            end: self.cursor(),
        };
    }

    fn transpose(&mut self) {
        self.textarea.cancel_selection();
        let (row, col) = self.cursor();
        let line: Vec<char> = self.textarea.lines()[row].chars().collect();
        // At the end of a line, Emacs swaps the two characters before
        // point; elsewhere, the characters either side of it.
        let at = if col >= line.len() {
            col.saturating_sub(1)
        } else {
            col
        };
        if at == 0 || line.len() < 2 {
            self.message = Some("Beginning of line".into());
            return;
        }
        let swapped: String = [line[at], line[at - 1]].iter().collect();
        self.jump((row, at - 1));
        self.textarea.delete_str(2);
        self.textarea.insert_str(swapped);
    }

    fn case_word(&mut self, change: impl Fn(&str) -> String) {
        self.textarea.cancel_selection();
        // The next word, on this line or a later one, as forward-word
        // finds it.
        let (row, end) = self.word_target(Direction::Forward);
        let line: Vec<char> = self.textarea.lines()[row].chars().collect();
        let len = line[..end]
            .iter()
            .rev()
            .take_while(|c| is_word(**c))
            .count();
        if len == 0 {
            return;
        }
        let start = end - len;
        let word: String = line[start..end].iter().collect();
        self.jump((row, start));
        self.textarea.delete_str(len);
        self.textarea.insert_str(change(&word));
    }

    fn search_key(&mut self, event: &KeyEvent) -> Outcome {
        match self.keymap.feed(event) {
            Lookup::Ignored | Lookup::Pending => Outcome::Continue,
            Lookup::Action(E::IsearchForward) => {
                self.search_again(true);
                Outcome::Continue
            }
            Lookup::Action(E::IsearchBackward) => {
                self.search_again(false);
                Outcome::Continue
            }
            Lookup::Action(E::DeleteBackwardChar) => {
                self.search_shrink();
                Outcome::Continue
            }
            Lookup::Action(E::KeyboardQuit) | Lookup::Cancelled => {
                self.search_abort();
                Outcome::Continue
            }
            Lookup::Action(E::Newline) => {
                self.search_finish();
                Outcome::Continue
            }
            Lookup::Unbound(key) if key.printable().is_some() => {
                if let (Some(c), Mode::Search(s)) = (key.printable(), &mut self.mode) {
                    s.query.push(c);
                }
                let from = self.search_state().match_start;
                self.search_from(from);
                Outcome::Continue
            }
            other => {
                self.search_finish();
                self.dispatch(other)
            }
        }
    }

    fn search_state(&self) -> Search {
        match &self.mode {
            Mode::Search(s) => s.clone(),
            _ => unreachable!("search_state outside a search"),
        }
    }

    fn set_search(&mut self, s: Search) {
        self.mode = Mode::Search(s);
    }

    fn pattern(query: &str) -> String {
        let escaped = regex::escape(query);
        if query.chars().any(char::is_uppercase) {
            escaped
        } else {
            format!("(?i){escaped}")
        }
    }

    /// Point after a match: its end going forward, its start going back.
    fn land(&mut self, s: &mut Search) {
        s.match_start = self.cursor();
        s.failing = false;
        if s.forward {
            let (row, col) = s.match_start;
            self.jump((row, col + s.query.chars().count()));
        }
    }

    /// One step of the widget's search from point. The widget's
    /// backward search skips its current line when point is at the end
    /// of it, so a backward step from there starts on the last
    /// character instead, which finds the same matches.
    fn find(&mut self, forward: bool, match_cursor: bool) -> bool {
        if forward {
            return self.textarea.search_forward(match_cursor);
        }
        let (row, col) = self.cursor();
        let len = self.textarea.lines()[row].chars().count();
        if len > 0 && col >= len {
            self.jump((row, len - 1));
            return self.textarea.search_back(true);
        }
        self.textarea.search_back(match_cursor)
    }

    fn search_from(&mut self, from: Pos) {
        let mut s = self.search_state();
        let here = self.cursor();
        if self
            .textarea
            .set_search_pattern(Self::pattern(&s.query))
            .is_err()
        {
            // An escaped query always compiles; if one ever does not,
            // the search reports a failure rather than standing still.
            s.failing = true;
            self.set_search(s);
            return;
        }
        self.jump(from);
        if self.find(s.forward, true) {
            self.land(&mut s);
        } else {
            s.failing = true;
            self.jump(here);
        }
        self.set_search(s);
    }

    fn search_again(&mut self, forward: bool) {
        let mut s = self.search_state();
        if s.query.is_empty() {
            if self.last_search.is_empty() {
                return;
            }
            s.query.clone_from(&self.last_search);
            s.forward = forward;
            let from = s.match_start;
            self.set_search(s);
            self.search_from(from);
            return;
        }
        let here = self.cursor();
        s.forward = forward;
        self.jump(s.match_start);
        if self.find(forward, false) {
            self.land(&mut s);
        } else {
            s.failing = true;
            self.jump(here);
        }
        self.set_search(s);
    }

    fn search_shrink(&mut self) {
        let mut s = self.search_state();
        s.query.pop();
        if s.query.is_empty() {
            let _ = self.textarea.set_search_pattern("");
            self.jump(s.origin);
            s.match_start = s.origin;
            s.failing = false;
            self.set_search(s);
        } else {
            let origin = s.origin;
            self.set_search(s);
            self.search_from(origin);
        }
    }

    fn search_finish(&mut self) {
        let s = self.search_state();
        if !s.query.is_empty() {
            self.last_search = s.query;
        }
        let _ = self.textarea.set_search_pattern("");
        self.mark = Some(s.origin);
        self.mode = Mode::Edit;
        self.message = None;
    }

    fn search_abort(&mut self) {
        let s = self.search_state();
        let _ = self.textarea.set_search_pattern("");
        self.jump(s.origin);
        self.mode = Mode::Edit;
        self.message = Some("Quit".into());
    }
}

impl Widget for &Editor {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.single_line || area.height < 2 {
            (&self.textarea).render(area, buf);
            return;
        }
        let text = Rect {
            height: area.height - 1,
            ..area
        };
        let echo = Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        };
        (&self.textarea).render(text, buf);
        if let Some(line) = self.echo() {
            Line::raw(line).render(echo, buf);
        }
    }
}

#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;
