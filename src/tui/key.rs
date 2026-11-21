//! One key, written in Emacs's key notation.
//!
//! The notation is Emacs's strict `kbd` syntax, the form `key-valid-p`
//! accepts: modifiers `C-` `M-` `S-` `s-` in that order, the named keys
//! `RET` `TAB` `ESC` `SPC` `DEL`, other named keys in angle brackets
//! (`<up>`, `<f1>`), and a printable character as itself. A sequence is
//! keys separated by one space: `C-x C-s`.
//!
//! A terminal cannot send every key Emacs can name, so a key is
//! normalised the same way when it is parsed and when it arrives:
//!
//! | Written         | Arrives from the terminal as      | Key        |
//! | --------------- | --------------------------------- | ---------- |
//! | `C-/`, `C-_`    | 0x1F, which crossterm reads as Ctrl+7 | `C-/`  |
//! | `C-SPC`, `C-@`  | NUL, read as Ctrl+Space           | `C-SPC`    |
//! | `C-\`           | 0x1C, read as Ctrl+4              | `C-\`      |
//! | `C-]`           | 0x1D, read as Ctrl+5              | `C-]`      |
//! | `C-^`           | 0x1E, read as Ctrl+6              | `C-^`      |
//! | `C-i`, `C-m`, `C-[` | the TAB, RET, and ESC bytes   | `TAB` `RET` `ESC` |
//!
//! `C-?` is refused: it is the DEL byte, and a terminal without the
//! keyboard enhancement protocol cannot tell it from the backspace key.
//! Shift on a printable character is carried by the character, as in
//! Emacs, so `G` and `<` are written as themselves and a terminal's
//! shift bit on them is dropped.

use std::fmt;

use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// One key with its modifiers, normalised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
    pub meta: bool,
    pub shift: bool,
    pub super_key: bool,
}

/// The named keys in angle brackets, with the code each names.
const BRACKETED: &[(&str, KeyCode)] = &[
    ("up", KeyCode::Up),
    ("down", KeyCode::Down),
    ("left", KeyCode::Left),
    ("right", KeyCode::Right),
    ("home", KeyCode::Home),
    ("end", KeyCode::End),
    ("prior", KeyCode::PageUp),
    ("next", KeyCode::PageDown),
    ("insert", KeyCode::Insert),
    ("delete", KeyCode::Delete),
    ("backspace", KeyCode::Backspace),
    ("backtab", KeyCode::BackTab),
];

/// The modifiers, in the order Emacs writes them.
const MODIFIERS: &[char] = &['C', 'M', 'S', 's'];

impl Key {
    /// A key with no modifiers.
    #[must_use]
    pub const fn plain(code: KeyCode) -> Self {
        Self {
            code,
            ctrl: false,
            meta: false,
            shift: false,
            super_key: false,
        }
    }

    /// The key a terminal event names, or `None` for a release, a
    /// lone modifier, or a code that names no key.
    #[must_use]
    pub fn from_event(event: &KeyEvent) -> Option<Self> {
        if event.kind == KeyEventKind::Release {
            return None;
        }
        if matches!(event.code, KeyCode::Modifier(_) | KeyCode::Null) {
            return None;
        }
        let m = event.modifiers;
        let key = Self {
            code: event.code,
            ctrl: m.contains(KeyModifiers::CONTROL),
            meta: m.intersects(KeyModifiers::ALT | KeyModifiers::META),
            shift: m.contains(KeyModifiers::SHIFT),
            super_key: m.contains(KeyModifiers::SUPER),
        };
        Some(key.normalised())
    }

    /// The same key with the meta modifier added.
    #[must_use]
    pub const fn with_meta(mut self) -> Self {
        self.meta = true;
        self
    }

    /// A printable character with no control or meta modifier: a key
    /// an editor inserts when nothing binds it.
    #[must_use]
    pub const fn printable(&self) -> Option<char> {
        match self.code {
            KeyCode::Char(c) if !self.ctrl && !self.meta && !self.super_key => Some(c),
            _ => None,
        }
    }

    /// Apply the table in the module docs.
    fn normalised(mut self) -> Self {
        match self.code {
            KeyCode::Char(c) => {
                self.shift = false;
                if self.ctrl {
                    self.code = KeyCode::Char(match c {
                        '7' | '_' => '/',
                        '@' => ' ',
                        '4' => '\\',
                        '5' => ']',
                        '6' => '^',
                        other => other,
                    });
                }
            }
            KeyCode::BackTab => self.shift = false,
            _ => {}
        }
        self
    }
}

/// Why a key or sequence did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseKeyError(pub String);

impl fmt::Display for ParseKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseKeyError {}

fn err(msg: impl Into<String>) -> ParseKeyError {
    ParseKeyError(msg.into())
}

impl std::str::FromStr for Key {
    type Err = ParseKeyError;

    fn from_str(token: &str) -> Result<Self, Self::Err> {
        if token.is_empty() {
            return Err(err("empty key"));
        }
        let mut key = Self::plain(KeyCode::Null);
        let mut rest = token;
        let mut last_modifier: Option<usize> = None;
        // A modifier is one letter and a hyphen with something after it,
        // so `-` and `C--` still name the hyphen key.
        while rest.len() >= 3 && rest.as_bytes()[1] == b'-' {
            let letter = rest.as_bytes()[0] as char;
            let Some(rank) = MODIFIERS.iter().position(|&m| m == letter) else {
                if letter == 'A' || letter == 'H' {
                    return Err(err(format!(
                        "{token}: a terminal cannot send the {letter}- modifier"
                    )));
                }
                break;
            };
            if last_modifier.is_some_and(|last| rank <= last) {
                return Err(err(format!(
                    "{token}: write modifiers once each, in the order C- M- S- s-"
                )));
            }
            last_modifier = Some(rank);
            match letter {
                'C' => key.ctrl = true,
                'M' => key.meta = true,
                'S' => key.shift = true,
                _ => key.super_key = true,
            }
            rest = &rest[2..];
        }
        key.code = parse_name(token, rest)?;
        if let KeyCode::Char(c) = key.code {
            if key.shift {
                return Err(err(format!(
                    "{token}: shift on a character is the character itself; write {}",
                    c.to_uppercase()
                )));
            }
            if key.ctrl {
                match c {
                    '?' => {
                        return Err(err(format!(
                            "{token}: a terminal sends C-? as DEL; bind DEL instead"
                        )));
                    }
                    'i' => {
                        return Ok(Self {
                            code: KeyCode::Tab,
                            ctrl: false,
                            ..key
                        });
                    }
                    'm' => {
                        return Ok(Self {
                            code: KeyCode::Enter,
                            ctrl: false,
                            ..key
                        });
                    }
                    '[' => {
                        return Ok(Self {
                            code: KeyCode::Esc,
                            ctrl: false,
                            ..key
                        });
                    }
                    _ => {}
                }
            }
        }
        Ok(key.normalised())
    }
}

fn parse_name(token: &str, name: &str) -> Result<KeyCode, ParseKeyError> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Ok(KeyCode::Char(c));
    }
    match name {
        "RET" => return Ok(KeyCode::Enter),
        "TAB" => return Ok(KeyCode::Tab),
        "ESC" => return Ok(KeyCode::Esc),
        "SPC" => return Ok(KeyCode::Char(' ')),
        "DEL" => return Ok(KeyCode::Backspace),
        _ => {}
    }
    let inner = name
        .strip_prefix('<')
        .and_then(|n| n.strip_suffix('>'))
        .ok_or_else(|| err(format!("{token}: unknown key {name}")))?;
    if let Some((_, code)) = BRACKETED.iter().find(|(n, _)| *n == inner) {
        return Ok(*code);
    }
    if let Some(n) = inner.strip_prefix('f').and_then(|d| d.parse::<u8>().ok())
        && (1..=24).contains(&n)
    {
        return Ok(KeyCode::F(n));
    }
    Err(err(format!("{token}: unknown key <{inner}>")))
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("C-")?;
        }
        if self.meta {
            f.write_str("M-")?;
        }
        if self.shift {
            f.write_str("S-")?;
        }
        if self.super_key {
            f.write_str("s-")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("SPC"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Enter => f.write_str("RET"),
            KeyCode::Tab => f.write_str("TAB"),
            KeyCode::Esc => f.write_str("ESC"),
            KeyCode::Backspace => f.write_str("DEL"),
            KeyCode::F(n) => write!(f, "<f{n}>"),
            code => match BRACKETED.iter().find(|(_, c)| *c == code) {
                Some((name, _)) => write!(f, "<{name}>"),
                None => write!(f, "<{code:?}>"),
            },
        }
    }
}

/// Parse a key sequence: keys separated by one space.
pub fn parse_sequence(text: &str) -> Result<Vec<Key>, ParseKeyError> {
    if text.is_empty() {
        return Err(err("empty key sequence"));
    }
    text.split(' ')
        .map(|token| {
            if token.is_empty() {
                Err(err(format!(
                    "{text:?}: separate keys with one space, with none at either end"
                )))
            } else {
                token.parse()
            }
        })
        .collect()
}

/// Write a key sequence the way it parses.
#[must_use]
pub fn sequence_to_string(keys: &[Key]) -> String {
    keys.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, modifiers: KeyModifiers) -> Key {
        Key::from_event(&KeyEvent::new(code, modifiers)).expect("a key")
    }

    fn parsed(s: &str) -> Key {
        s.parse().unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn every_written_form_round_trips() {
        for s in [
            "a",
            "G",
            "<",
            "?",
            "-",
            "C--",
            "C-x",
            "M-f",
            "C-M-f",
            "M-<",
            "M->",
            "RET",
            "TAB",
            "ESC",
            "SPC",
            "DEL",
            "C-SPC",
            "M-DEL",
            "<up>",
            "<down>",
            "<left>",
            "<right>",
            "<home>",
            "<end>",
            "<prior>",
            "<next>",
            "<insert>",
            "<delete>",
            "<backtab>",
            "<f1>",
            "<f12>",
            "C-<up>",
            "M-<delete>",
            "S-<up>",
            "s-a",
            "C-/",
            "C-\\",
            "C-]",
            "C-^",
            "M-ESC",
        ] {
            assert_eq!(parsed(s).to_string(), s, "{s}");
        }
    }

    #[test]
    fn aliases_parse_to_the_key_the_terminal_sends() {
        assert_eq!(parsed("C-_"), parsed("C-/"));
        assert_eq!(parsed("C-@"), parsed("C-SPC"));
        assert_eq!(parsed("C-i"), parsed("TAB"));
        assert_eq!(parsed("C-m"), parsed("RET"));
        assert_eq!(parsed("C-["), parsed("ESC"));
        assert_eq!(parsed("<backspace>"), parsed("DEL"));
    }

    #[test]
    fn control_bytes_arrive_as_the_keys_emacs_names() {
        let ctrl = KeyModifiers::CONTROL;
        assert_eq!(ev(KeyCode::Char('7'), ctrl), parsed("C-/"));
        assert_eq!(ev(KeyCode::Char('_'), ctrl), parsed("C-/"));
        assert_eq!(ev(KeyCode::Char(' '), ctrl), parsed("C-SPC"));
        assert_eq!(ev(KeyCode::Char('@'), ctrl), parsed("C-SPC"));
        assert_eq!(ev(KeyCode::Char('4'), ctrl), parsed("C-\\"));
        assert_eq!(ev(KeyCode::Char('5'), ctrl), parsed("C-]"));
        assert_eq!(ev(KeyCode::Char('6'), ctrl), parsed("C-^"));
        assert_eq!(
            ev(KeyCode::Char('7'), ctrl | KeyModifiers::ALT),
            parsed("C-M-/")
        );
    }

    #[test]
    fn shift_on_a_character_is_the_character() {
        let shift = KeyModifiers::SHIFT;
        assert_eq!(ev(KeyCode::Char('G'), shift), parsed("G"));
        assert_eq!(ev(KeyCode::Char('?'), shift), parsed("?"));
        assert_eq!(
            ev(KeyCode::Char('<'), shift | KeyModifiers::ALT),
            parsed("M-<")
        );
        assert_eq!(ev(KeyCode::BackTab, shift), parsed("<backtab>"));
        assert_eq!(ev(KeyCode::Up, shift), parsed("S-<up>"));
    }

    #[test]
    fn meta_is_alt_or_meta() {
        assert_eq!(ev(KeyCode::Char('f'), KeyModifiers::ALT), parsed("M-f"));
        assert_eq!(ev(KeyCode::Char('f'), KeyModifiers::META), parsed("M-f"));
    }

    #[test]
    fn a_release_and_a_lone_modifier_are_no_key() {
        let mut release = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(Key::from_event(&release), None);
        let null = KeyEvent::new(KeyCode::Null, KeyModifiers::NONE);
        assert_eq!(Key::from_event(&null), None);
    }

    #[test]
    fn malformed_keys_are_refused_with_a_reason() {
        for (s, says) in [
            ("", "empty"),
            ("ctrl+x", "unknown key"),
            ("<pgup>", "unknown key <pgup>"),
            ("<f25>", "unknown key <f25>"),
            ("M-C-x", "order"),
            ("C-C-x", "order"),
            ("S-a", "write A"),
            ("C-?", "DEL"),
            ("A-x", "cannot send"),
            ("H-x", "cannot send"),
            ("Return", "unknown key"),
        ] {
            let e = s.parse::<Key>().expect_err(s).0;
            assert!(e.contains(says), "{s}: {e}");
        }
    }

    #[test]
    fn a_sequence_is_keys_separated_by_one_space() {
        let seq = parse_sequence("C-x C-s").unwrap();
        assert_eq!(seq, vec![parsed("C-x"), parsed("C-s")]);
        assert_eq!(sequence_to_string(&seq), "C-x C-s");
        for bad in ["", "C-x  C-s", " C-x", "C-x "] {
            assert!(parse_sequence(bad).is_err(), "{bad:?}");
        }
    }
}
