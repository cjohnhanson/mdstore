//! The pieces every served store shares.
//!
//! A tool has three faces: the library, the command line, and a server.
//! The library holds the behavior. This module holds what the server
//! face needs and the command line does not: how a document is named by
//! a URI, which surfaces the server offers, and what a remote caller is
//! allowed to do.
//!
//! It carries no transport and no protocol codec. Each tool builds its
//! own server on its own library; this module keeps the parts that
//! would otherwise be written three times and drift apart.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::store::StoreRef;

// -- What the server offers --

/// One surface a server can expose.
///
/// A client cannot be asked which of these it understands. The protocol
/// declares `resources` and `tools` as *server* capabilities: the
/// server offers them, and a client that cannot use one simply never
/// calls it. A client declares only `roots`, `sampling`, `elicitation`,
/// and `experimental`, none of which say whether it reads resources.
///
/// So the choice is configuration, not negotiation. The operator knows
/// which clients connect; the server cannot find out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    /// Callable tools. Every client supports these, so this is the
    /// floor that a served store can rely on.
    Tools,
    /// Readable resources, addressed by URI. Cheaper for a client that
    /// supports them: it reads one document instead of calling a tool
    /// and parsing the result.
    Resources,
    /// The skills extension. The richest surface, and the newest, so
    /// the fewest clients implement it.
    Skills,
}

impl fmt::Display for Surface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Surface::Tools => "tools",
            Surface::Resources => "resources",
            Surface::Skills => "skills",
        })
    }
}

impl FromStr for Surface {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "tools" => Ok(Surface::Tools),
            "resources" => Ok(Surface::Resources),
            "skills" => Ok(Surface::Skills),
            other => Err(crate::Error::InvalidStore(format!(
                "'{other}' is not a surface; use tools, resources, or skills"
            ))),
        }
    }
}

/// The surfaces one server offers.
///
/// More than one can be on at once. A client uses the surface it
/// understands and ignores the rest. The cost of leaving several on is
/// context: a tool definition is sent to the model on every request, so
/// a deployment that knows its client can trim the set and get that
/// space back.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Surfaces(Vec<Surface>);

impl Surfaces {
    /// The surfaces, in a stable order.
    #[must_use]
    pub fn new(mut surfaces: Vec<Surface>) -> Self {
        surfaces.sort_unstable();
        surfaces.dedup();
        Surfaces(surfaces)
    }

    /// Tools alone: the set that every client can use.
    #[must_use]
    pub fn tools_only() -> Self {
        Surfaces::new(vec![Surface::Tools])
    }

    /// Resources for a client that reads them, and tools for one that
    /// does not.
    #[must_use]
    pub fn resources_and_tools() -> Self {
        Surfaces::new(vec![Surface::Resources, Surface::Tools])
    }

    /// The skills extension for a client that implements it, and tools
    /// for one that does not.
    #[must_use]
    pub fn skills_and_tools() -> Self {
        Surfaces::new(vec![Surface::Skills, Surface::Tools])
    }

    #[must_use]
    pub fn has(&self, surface: Surface) -> bool {
        self.0.contains(&surface)
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Surface] {
        &self.0
    }

    /// Parse a comma-separated list, as a flag or a config value takes
    /// it: `resources,tools`.
    pub fn parse_list(s: &str) -> crate::Result<Self> {
        let mut out = Vec::new();
        for part in s.split(',').filter(|p| !p.trim().is_empty()) {
            out.push(part.parse::<Surface>()?);
        }
        if out.is_empty() {
            return Err(crate::Error::InvalidStore(
                "a server must offer at least one surface".into(),
            ));
        }
        Ok(Surfaces::new(out))
    }
}

impl fmt::Display for Surfaces {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self.0.iter().map(ToString::to_string).collect();
        f.write_str(&names.join(","))
    }
}

// -- What a caller may do --

/// What a served store allows.
///
/// A served store is reachable by whoever can open the connection, so
/// the default is read-only. Writing is a decision the operator makes,
/// not a default they inherit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    /// Reads only. Every write is refused.
    #[default]
    ReadOnly,
    /// Reads, and the writes a remote caller may make. Approval is
    /// never among them: approving content is a human act, and the
    /// human has the command line.
    ReadWrite,
}

impl Access {
    #[must_use]
    pub const fn writable(self) -> bool {
        matches!(self, Access::ReadWrite)
    }
}

impl FromStr for Access {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "read-only" | "readonly" => Ok(Access::ReadOnly),
            "read-write" | "readwrite" => Ok(Access::ReadWrite),
            other => Err(crate::Error::InvalidStore(format!(
                "'{other}' is not an access mode; use read-only or read-write"
            ))),
        }
    }
}

/// How one store is served.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServeConfig {
    /// The store the server reads from. Every command runs from here,
    /// so this root also fixes what the server can reach: the stores
    /// this one declares, and nothing else.
    pub root: std::path::PathBuf,
    /// The surfaces to offer.
    pub surfaces: Surfaces,
    /// What a caller may do.
    #[serde(default)]
    pub access: Access,
    /// The name the server reports at initialize.
    pub server_name: String,
}

impl ServeConfig {
    /// A read-only server on one store.
    #[must_use]
    pub fn read_only(
        root: impl Into<std::path::PathBuf>,
        surfaces: Surfaces,
        server_name: impl Into<String>,
    ) -> Self {
        ServeConfig {
            root: root.into(),
            surfaces,
            access: Access::ReadOnly,
            server_name: server_name.into(),
        }
    }

    /// Refuse a write when the server is read-only.
    pub fn ensure_writable(&self) -> crate::Result<()> {
        if self.access.writable() {
            return Ok(());
        }
        Err(crate::Error::InvalidStore(
            "this store is served read-only".into(),
        ))
    }
}

// -- How a document is named --

/// A document URI: `<scheme>://<kind>/<qualified-id>`.
///
/// The qualified ID is what the command line prints, so an ID a person
/// reads in a terminal is the same ID a client puts in a URI, including
/// the store alias: `zettel://note/team:a3f2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocUri {
    pub scheme: String,
    /// What kind of document: `note`, `issue`, `skill`.
    pub kind: String,
    /// The reference, qualified by store alias when it is not local.
    pub reference: StoreRef,
    /// A file inside the document's directory, for a store that keeps
    /// several files for one document.
    pub sub_path: Option<String>,
}

impl DocUri {
    /// Build a URI for a document.
    pub fn new(scheme: &str, kind: &str, reference: StoreRef) -> Self {
        DocUri {
            scheme: scheme.to_string(),
            kind: kind.to_string(),
            reference,
            sub_path: None,
        }
    }

    /// Build a URI for a file inside a document.
    pub fn with_sub_path(mut self, sub_path: impl Into<String>) -> Self {
        self.sub_path = Some(sub_path.into());
        self
    }

    /// Parse a URI against the aliases the serving store declares.
    ///
    /// The alias table is the serving store's, because the URI is
    /// written by a client that sees that store's vantage.
    pub fn parse(uri: &str, declared: &dyn crate::store::AliasSet) -> crate::Result<Self> {
        let (scheme, rest) = uri
            .split_once("://")
            .ok_or_else(|| crate::Error::InvalidStore(format!("'{uri}' is not a document URI")))?;
        let (kind, tail) = rest
            .split_once('/')
            .ok_or_else(|| crate::Error::InvalidStore(format!("'{uri}' names no document")))?;
        if kind.is_empty() || tail.is_empty() {
            return Err(crate::Error::InvalidStore(format!(
                "'{uri}' names no document"
            )));
        }
        // A sub-path follows the document reference. The reference
        // itself can hold a colon for a store alias, so the split is on
        // the first slash after the reference.
        let (id_part, sub_path) = match tail.split_once('/') {
            Some((id, sub)) if !sub.is_empty() => (id, Some(sub.to_string())),
            _ => (tail, None),
        };
        Ok(DocUri {
            scheme: scheme.to_string(),
            kind: kind.to_string(),
            reference: StoreRef::parse(id_part, declared),
            sub_path,
        })
    }
}

impl fmt::Display for DocUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}/{}", self.scheme, self.kind, self.reference)?;
        if let Some(sub) = &self.sub_path {
            write!(f, "/{sub}")?;
        }
        Ok(())
    }
}

/// A SHA-256 digest in the form the skills extension asks for:
/// `sha256:<hex>`.
#[must_use]
pub fn digest_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let out = hasher.finalize();
    let hex: String = out.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::NoAliases;

    struct Aliases(Vec<String>);
    impl crate::store::AliasSet for Aliases {
        fn declares(&self, alias: &str) -> bool {
            self.0.iter().any(|a| a == alias)
        }
    }

    #[test]
    fn surfaces_parse_from_a_list() {
        let s = Surfaces::parse_list("resources,tools").unwrap();
        assert!(s.has(Surface::Resources));
        assert!(s.has(Surface::Tools));
        assert!(!s.has(Surface::Skills));
    }

    #[test]
    fn a_surface_list_is_ordered_and_deduplicated() {
        let s = Surfaces::parse_list("tools,resources,tools").unwrap();
        assert_eq!(s.as_slice().len(), 2);
        assert_eq!(s.to_string(), "tools,resources");
    }

    #[test]
    fn an_empty_surface_list_is_rejected() {
        assert!(Surfaces::parse_list("").is_err());
        assert!(Surfaces::parse_list(",,").is_err());
    }

    #[test]
    fn an_unknown_surface_is_rejected_by_name() {
        let err = Surfaces::parse_list("prompts").unwrap_err().to_string();
        assert!(err.contains("prompts"), "{err}");
    }

    #[test]
    fn the_defaults_keep_tools_as_the_floor() {
        // Every client can call a tool, so a served store stays usable
        // when the richer surface is not understood.
        assert!(Surfaces::resources_and_tools().has(Surface::Tools));
        assert!(Surfaces::skills_and_tools().has(Surface::Tools));
    }

    #[test]
    fn a_served_store_is_read_only_until_told_otherwise() {
        let config = ServeConfig::read_only("/srv/kb", Surfaces::tools_only(), "zettel");
        assert_eq!(config.access, Access::ReadOnly);
        assert!(config.ensure_writable().is_err());
    }

    #[test]
    fn a_writable_store_allows_a_write() {
        let mut config = ServeConfig::read_only("/srv/kb", Surfaces::tools_only(), "zettel");
        config.access = Access::ReadWrite;
        assert!(config.ensure_writable().is_ok());
    }

    #[test]
    fn access_parses_both_spellings() {
        assert_eq!("read-only".parse::<Access>().unwrap(), Access::ReadOnly);
        assert_eq!("readwrite".parse::<Access>().unwrap(), Access::ReadWrite);
        assert!("anything".parse::<Access>().is_err());
    }

    #[test]
    fn a_local_document_uri_round_trips() {
        let uri = DocUri::new("zettel", "note", StoreRef::local("a3f2"));
        assert_eq!(uri.to_string(), "zettel://note/a3f2");
        let parsed = DocUri::parse(&uri.to_string(), &NoAliases).unwrap();
        assert_eq!(parsed, uri);
    }

    #[test]
    fn a_qualified_document_uri_keeps_its_store() {
        let aliases = Aliases(vec!["team".into()]);
        let reference = StoreRef::parse("team:a3f2", &aliases);
        let uri = DocUri::new("zettel", "note", reference);
        assert_eq!(uri.to_string(), "zettel://note/team:a3f2");
        let parsed = DocUri::parse(&uri.to_string(), &aliases).unwrap();
        assert_eq!(parsed.reference.alias, vec!["team".to_string()]);
        assert_eq!(parsed.reference.id, "a3f2");
    }

    #[test]
    fn an_undeclared_alias_stays_part_of_the_id() {
        // The discrimination rule holds here too: without the alias
        // declared, the text is just an ID.
        let parsed = DocUri::parse("zettel://note/team:a3f2", &NoAliases).unwrap();
        assert!(!parsed.reference.is_foreign());
        assert_eq!(parsed.reference.id, "team:a3f2");
    }

    #[test]
    fn a_uri_can_name_a_file_inside_a_document() {
        let uri = DocUri::new("skill", "skill", StoreRef::local("writing"))
            .with_sub_path("references/deep-dive.md");
        assert_eq!(
            uri.to_string(),
            "skill://skill/writing/references/deep-dive.md"
        );
        let parsed = DocUri::parse(&uri.to_string(), &NoAliases).unwrap();
        assert_eq!(parsed.sub_path.as_deref(), Some("references/deep-dive.md"));
        assert_eq!(parsed.reference.id, "writing");
    }

    #[test]
    fn a_malformed_uri_is_rejected() {
        for bad in ["not-a-uri", "zettel://", "zettel://note/", "zettel://note"] {
            assert!(DocUri::parse(bad, &NoAliases).is_err(), "{bad} must fail");
        }
    }

    #[test]
    fn digests_match_the_known_vectors() {
        // The skills extension compares these, so a wrong digest means
        // a client rejects content that is correct.
        assert_eq!(
            digest_of(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            digest_of(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_digest_covers_a_long_input() {
        // Longer than one block, so the padding path is exercised.
        let text = "a".repeat(1000);
        let digest = digest_of(text.as_bytes());
        assert_eq!(
            digest,
            "sha256:41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }
}

// -- Tool arguments --

/// One optional string argument from a tool call.
///
/// Three answers, never two: absent, present as a string, or present
/// as something else. Reading it with `as_str().map(...)` collapses
/// the third into the first, so a filter of the wrong type was
/// dropped and the caller got unfiltered data marked as success.
///
/// The name of the tool is not needed here: a wrong type is a caller
/// mistake about this argument, whichever tool holds it.
pub fn optional_str<'a>(
    args: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> std::result::Result<Option<&'a str>, ArgError> {
    match args.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(v)) => Ok(Some(v.as_str())),
        Some(_) => Err(ArgError::WrongType {
            key: key.to_string(),
        }),
    }
}

/// One required string argument from a tool call.
pub fn required_str<'a>(
    args: &'a serde_json::Map<String, serde_json::Value>,
    tool: &str,
    key: &str,
) -> std::result::Result<&'a str, ArgError> {
    match optional_str(args, key)? {
        Some(v) => Ok(v),
        None => Err(ArgError::Missing {
            tool: tool.to_string(),
            key: key.to_string(),
        }),
    }
}

/// What was wrong with a tool argument.
///
/// A missing argument and a mistyped one are different mistakes, and
/// reporting either as "not found" sends the caller looking for the
/// wrong problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    Missing { tool: String, key: String },
    WrongType { key: String },
}

impl std::fmt::Display for ArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { tool, key } => write!(f, "{tool} needs the '{key}' argument"),
            Self::WrongType { key } => write!(f, "'{key}' must be a string"),
        }
    }
}

impl std::error::Error for ArgError {}

#[cfg(test)]
mod argument_tests {
    use super::*;
    use serde_json::json;

    fn args(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn an_optional_argument_has_three_answers() {
        let a = args(json!({"tag": "bug", "count": 3, "nothing": null}));
        assert_eq!(optional_str(&a, "tag").unwrap(), Some("bug"));
        assert_eq!(optional_str(&a, "absent").unwrap(), None);
        assert_eq!(optional_str(&a, "nothing").unwrap(), None);
        assert!(optional_str(&a, "count").is_err());
    }

    #[test]
    fn a_missing_argument_is_not_a_wrong_one() {
        let a = args(json!({"count": 3}));
        let missing = required_str(&a, "the_tool", "id").unwrap_err();
        assert!(format!("{missing}").contains("needs the 'id' argument"));
        let wrong = required_str(&a, "the_tool", "count").unwrap_err();
        assert!(format!("{wrong}").contains("must be a string"));
    }
}

// -- Network exposure --

/// True when the bind address listens only on this machine.
///
/// A served store answers whoever reaches it. On a loopback address
/// that is the person at the keyboard. On any other address it is the
/// network, and the store has no authentication of its own.
#[must_use]
pub fn addr_is_loopback(addr: &str) -> bool {
    // A bare IPv6 address holds colons of its own, so try the whole
    // string before splitting a port off it.
    if let Ok(ip) = addr.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    let host = match addr.rsplit_once(':') {
        // [::1]:7431 and 127.0.0.1:7431
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => h,
        _ => addr,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// True when a browser origin belongs to this machine.
///
/// A page on the open web can post to a local server, and the reply
/// stays inside the browser only if the server refuses the request.
/// A request with no `Origin` header is not a browser request, so a
/// caller that sends none is left alone.
#[must_use]
pub fn origin_is_local(origin: &str) -> bool {
    let rest = match origin.split_once("://") {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("http") => rest,
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("https") => rest,
        _ => return false,
    };
    if let Ok(ip) = rest.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    let host = match rest.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => h,
        _ => rest,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

#[cfg(test)]
mod exposure_tests {
    use super::*;

    #[test]
    fn a_loopback_bind_is_recognized_in_every_spelling() {
        assert!(addr_is_loopback("127.0.0.1:7431"));
        assert!(addr_is_loopback("127.0.0.1"));
        assert!(addr_is_loopback("[::1]:7431"));
        assert!(addr_is_loopback("::1"));
        assert!(addr_is_loopback("127.5.5.5:80"));

        assert!(!addr_is_loopback("0.0.0.0:7431"));
        assert!(!addr_is_loopback("192.168.1.10:7431"));
        assert!(!addr_is_loopback("[::]:7431"));
        assert!(!addr_is_loopback("example.com:7431"));
    }

    #[test]
    fn only_a_local_origin_is_local() {
        assert!(origin_is_local("http://localhost:3000"));
        assert!(origin_is_local("http://127.0.0.1:8080"));
        assert!(origin_is_local("http://[::1]:8080"));
        assert!(origin_is_local("https://localhost"));

        assert!(!origin_is_local("http://evil.example.com"));
        assert!(!origin_is_local("https://localhost.evil.com"));
        assert!(!origin_is_local("file://"));
        assert!(!origin_is_local("null"));
        assert!(!origin_is_local(""));
    }
}
