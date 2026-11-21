//! The terminal UI kit the tools share, behind the `tui` feature.
//!
//! Two parts, so no tool builds its own:
//!
//! - [`keymap`]: key bindings read from `~/.config/<tool>/keys.yml`,
//!   laid over a built-in preset, `default` or `emacs`. Keys are written
//!   in Emacs's notation ([`key`]).
//! - [`editor`]: the markdown editor a tool opens on a document body,
//!   driven by the `editor` section of the same file. The tool saves
//!   what it returns through its own core operation.
//!
//! The kit holds no terminal and runs no event loop. A tool feeds it
//! key events and renders its widget.

pub mod editor;
pub mod key;
pub mod keymap;
pub mod kill_ring;

pub use editor::{EditAction, Editor, Outcome};
pub use key::{Key, ParseKeyError};
pub use keymap::{Binding, Command, Keymap, KeymapError, KeysFile, Lookup, Preset, Section};
