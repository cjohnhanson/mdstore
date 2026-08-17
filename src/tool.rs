//! The name of a consuming tool, validated once.
//!
//! Five functions build a path from this name: the user config's path,
//! its read and its write, the registry's path, and the registry's read.
//! A `&str` parameter on each puts the validation next to a caller who
//! must remember it, which is the shape `StoreContent` spent five review
//! rounds removing. A name that cannot be built wrong cannot reach them.
//!
//! `new` is `const`, so a consumer rejects a bad literal at compile
//! time:
//!
//! ```
//! use mdstore::ToolName;
//! const TOOL: ToolName<'static> = match ToolName::new("zettel") {
//!     Some(t) => t,
//!     None => panic!("the tool name must be one plain path component"),
//! };
//! assert_eq!(TOOL.as_str(), "zettel");
//! ```
//!
//! A `const` item with a bad literal fails to compile, and it fails
//! whether or not anything reads it:
//!
//! ```compile_fail
//! use mdstore::ToolName;
//! const BAD: ToolName<'static> = match ToolName::new("../escape") {
//!     Some(t) => t,
//!     None => panic!("the tool name must be one plain path component"),
//! };
//! ```
//!
//! A `let` binding with the same match compiles and panics at run time
//! instead, so the guarantee belongs to the `const` shape above.
//!
//! A caller that arrives with a runtime name gets `None` and decides
//! what to do. Publication is what creates that caller.

/// A tool name that is one plain path component.
///
/// Borrowed, because a caller after publication may hold a `String`
/// rather than a literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolName<'a>(&'a str);

impl<'a> ToolName<'a> {
    /// The name, or `None` when it is not one plain path component.
    ///
    /// Rejects an empty name, `.`, `..`, any hidden name, and anything
    /// holding a path separator or a NUL, through `is_plain_stem`. Adds
    /// two rules of its own, because this name becomes a path component
    /// on every platform and is printed in commands:
    ///
    /// - a leading `-`, which reads as a flag;
    /// - a `:`, because Windows treats a component carrying a drive
    ///   prefix as a fresh root, so `Path::push` would discard the base
    ///   and `"C:"` would escape the home.
    ///
    /// Never normalises: a rejected name is the caller's to fix.
    #[must_use]
    pub const fn new(name: &'a str) -> Option<Self> {
        if !crate::store::is_plain_stem(name) {
            return None;
        }
        let b = name.as_bytes();
        if b[0] == b'-' {
            return None;
        }
        let mut i = 0;
        while i < b.len() {
            if b[i] == b':' {
                return None;
            }
            i += 1;
        }
        Some(ToolName(name))
    }

    /// The name as text, for a path component or a message.
    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.0
    }
}

impl std::fmt::Display for ToolName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_is_not_one_plain_component_is_refused() {
        for good in ["zettel", "tisket", "almanac", "a", "with-dash", "u2"] {
            assert!(ToolName::new(good).is_some(), "{good}");
            assert_eq!(ToolName::new(good).unwrap().as_str(), good);
        }
        // Each of these builds a path that leaves the directory the
        // caller intended, or collides with another tool's file.
        for bad in [
            "",
            ".",
            "..",
            "../../etc",
            "/etc/cron.d",
            "a/b",
            "a\\b",
            ".hidden",
            "with\0nul",
            "-flag",
            "a/",
            "C:",
            "c:name",
        ] {
            assert!(ToolName::new(bad).is_none(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn the_constructor_runs_in_a_const() {
        const T: Option<ToolName<'static>> = ToolName::new("zettel");
        const BAD: Option<ToolName<'static>> = ToolName::new("../escape");
        assert!(T.is_some());
        assert!(BAD.is_none());
    }
}
