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
    /// Rejects an empty name, `.`, `..`, any hidden name, anything
    /// holding a path separator or a NUL, and a leading `-`, which would
    /// read as a flag wherever the name is printed in a command. Never
    /// normalises: a rejected name is the caller's to fix.
    #[must_use]
    pub const fn new(name: &'a str) -> Option<Self> {
        if !crate::store::is_plain_stem(name) {
            return None;
        }
        if name.as_bytes()[0] == b'-' {
            return None;
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
