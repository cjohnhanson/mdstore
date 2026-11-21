//! Emacs's kill ring, for one editor.
//!
//! Killed text goes on the ring. A kill straight after another kill
//! joins the newest entry instead of starting one: appended for a
//! forward kill, prepended for a backward one, so `C-k C-k` yanks back
//! as one piece. `C-y` yanks the newest entry; `M-y` straight after a
//! yank steps to the one before it.

/// How many entries the ring keeps. Emacs keeps 120; a document body in
/// a terminal is short, and the value can change without a format bump.
pub const CAPACITY: usize = 60;

/// Which way a kill went from point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

/// The ring. The newest entry is last.
#[derive(Debug, Clone, Default)]
pub struct KillRing {
    entries: Vec<String>,
    /// How far `M-y` has stepped back from the newest entry.
    offset: usize,
}

impl KillRing {
    /// Put killed text on the ring. `join` is true when the command
    /// before this one was also a kill.
    pub fn kill(&mut self, text: String, direction: Direction, join: bool) {
        if text.is_empty() {
            return;
        }
        self.offset = 0;
        match (join, self.entries.last_mut()) {
            (true, Some(last)) => match direction {
                Direction::Forward => last.push_str(&text),
                Direction::Backward => last.insert_str(0, &text),
            },
            _ => {
                self.entries.push(text);
                if self.entries.len() > CAPACITY {
                    self.entries.remove(0);
                }
            }
        }
    }

    /// The text `C-y` inserts: the newest entry.
    pub fn yank(&mut self) -> Option<&str> {
        self.offset = 0;
        self.entries.last().map(String::as_str)
    }

    /// The text `M-y` swaps in: one entry older than the last yank,
    /// wrapping to the newest after the oldest.
    pub fn pop(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        self.offset = (self.offset + 1) % self.entries.len();
        let i = self.entries.len() - 1 - self.offset;
        Some(self.entries[i].as_str())
    }

    /// Whether the ring holds anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_forward_kills_append() {
        let mut ring = KillRing::default();
        ring.kill("one".into(), Direction::Forward, false);
        ring.kill("\n".into(), Direction::Forward, true);
        assert_eq!(ring.yank(), Some("one\n"));
    }

    #[test]
    fn consecutive_backward_kills_prepend() {
        let mut ring = KillRing::default();
        ring.kill("word".into(), Direction::Backward, false);
        ring.kill("two ".into(), Direction::Backward, true);
        assert_eq!(ring.yank(), Some("two word"));
    }

    #[test]
    fn a_kill_after_another_command_starts_an_entry() {
        let mut ring = KillRing::default();
        ring.kill("a".into(), Direction::Forward, false);
        ring.kill("b".into(), Direction::Forward, false);
        assert_eq!(ring.yank(), Some("b"));
        assert_eq!(ring.pop(), Some("a"));
        assert_eq!(ring.pop(), Some("b"), "pop wraps to the newest");
    }

    #[test]
    fn an_empty_kill_changes_nothing() {
        let mut ring = KillRing::default();
        ring.kill(String::new(), Direction::Forward, false);
        assert!(ring.is_empty());
        assert_eq!(ring.yank(), None);
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn the_ring_keeps_the_newest_entries() {
        let mut ring = KillRing::default();
        for i in 0..=CAPACITY {
            ring.kill(i.to_string(), Direction::Forward, false);
        }
        assert_eq!(ring.entries.len(), CAPACITY);
        assert_eq!(ring.entries[0], "1");
    }
}
