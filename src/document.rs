use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

/// A markdown document with YAML frontmatter.
#[derive(Debug)]
pub struct Document<T> {
    pub frontmatter: T,
    pub body: String,
}

/// Split a document into its raw frontmatter text and its body.
///
/// The frontmatter comes back as it was written, so a caller that must
/// hand it on unchanged does not re-serialize it. The skills extension
/// asks for verbatim frontmatter, and a client compares what a listing
/// gave against what a fetch gave.
pub fn split(content: &str) -> Result<(&str, &str)> {
    let (yaml, body) = split_fences(content)?;
    Ok((yaml.trim_end(), body))
}

/// Find the two fences and return the YAML between them and the body after.
///
/// The closing fence is a line that holds `---` and nothing else. A
/// `---` inside a value is text, not a fence: a title such as
/// `Pooling --- causes stale reads` is a legal scalar, and cutting the
/// frontmatter there loses every key after it.
///
/// One helper serves both `split` and `parse`, so the verbatim text and
/// the typed frontmatter can never disagree about where a document
/// ends.
fn split_fences(content: &str) -> Result<(&str, &str)> {
    let content = content.trim();
    let rest = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))
        .or_else(|| (content == "---").then_some(""))
        .ok_or(Error::MissingFrontmatter)?;

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Ok((
                &rest[..offset],
                rest[offset + line.len()..].trim_start_matches('\n'),
            ));
        }
        offset += line.len();
    }
    Err(Error::UnclosedFrontmatter)
}

/// Parse a `---`-fenced YAML frontmatter document into a typed frontmatter and body.
pub fn parse<T: DeserializeOwned>(content: &str) -> Result<Document<T>> {
    let (yaml, body) = split_fences(content)?;
    let frontmatter: T = yaml_serde::from_str(yaml)?;

    Ok(Document {
        frontmatter,
        body: body.trim().to_string(),
    })
}

/// Serialize a document back to `---`-fenced YAML frontmatter + body.
///
/// Uses `yaml_serde` for frontmatter serialization, producing canonical YAML output.
/// Tools that need specific field ordering or formatting should implement their own
/// serializer on top of this.
pub fn serialize<T: Serialize>(doc: &Document<T>) -> Result<String> {
    let yaml = yaml_serde::to_string(&doc.frontmatter)?;
    let mut out = String::from("---\n");
    out.push_str(&yaml);
    out.push_str("---\n");
    if !doc.body.is_empty() {
        out.push('\n');
        out.push_str(&doc.body);
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct TestFrontmatter {
        title: String,
        #[serde(default)]
        tags: Vec<String>,
    }

    #[test]
    fn parse_basic_document() {
        let content = "---\ntitle: \"Hello\"\ntags: [a, b]\n---\n\nSome body text.";
        let doc: Document<TestFrontmatter> = parse(content).unwrap();
        assert_eq!(doc.frontmatter.title, "Hello");
        assert_eq!(doc.frontmatter.tags, vec!["a", "b"]);
        assert_eq!(doc.body, "Some body text.");
    }

    #[test]
    fn parse_empty_body() {
        let content = "---\ntitle: \"Hello\"\n---\n";
        let doc: Document<TestFrontmatter> = parse(content).unwrap();
        assert_eq!(doc.frontmatter.title, "Hello");
        assert!(doc.body.is_empty());
    }

    #[test]
    fn parse_missing_frontmatter() {
        let content = "Just some text";
        let result = parse::<TestFrontmatter>(content);
        assert!(result.is_err());
    }

    #[test]
    fn parse_unclosed_frontmatter() {
        let content = "---\ntitle: \"Hello\"\n";
        let result = parse::<TestFrontmatter>(content);
        assert!(result.is_err());
    }

    #[test]
    fn split_gives_the_frontmatter_as_written() {
        let content = "---\nname: probe\ndescription: \"a: b\"\n---\n\nBody here.\n";
        let (frontmatter, body) = split(content).unwrap();
        assert_eq!(frontmatter, "name: probe\ndescription: \"a: b\"");
        assert_eq!(body, "Body here.");
    }

    #[test]
    fn split_rejects_what_parse_rejects() {
        assert!(split("no frontmatter").is_err());
        assert!(split("---\nunclosed: yes\n").is_err());
    }

    #[test]
    fn split_and_parse_agree_on_the_body() {
        let content = "---\ntitle: t\n---\n\nOne\n\nTwo\n";
        let (_, split_body) = split(content).unwrap();
        let doc: Document<TestFrontmatter> = parse(content).unwrap();
        assert_eq!(split_body.trim(), doc.body);
    }

    #[test]
    fn serialize_roundtrip() {
        let doc = Document {
            frontmatter: TestFrontmatter {
                title: "Test".into(),
                tags: vec!["x".into()],
            },
            body: "Body here.".into(),
        };
        let serialized = serialize(&doc).unwrap();
        let parsed: Document<TestFrontmatter> = parse(&serialized).unwrap();
        assert_eq!(parsed.frontmatter, doc.frontmatter);
        assert_eq!(parsed.body, doc.body);
    }

    #[test]
    fn serialize_empty_body() {
        let doc = Document {
            frontmatter: TestFrontmatter {
                title: "No body".into(),
                tags: vec![],
            },
            body: String::new(),
        };
        let serialized = serialize(&doc).unwrap();
        assert!(serialized.ends_with("---\n"));
        let parsed: Document<TestFrontmatter> = parse(&serialized).unwrap();
        assert_eq!(parsed.frontmatter.title, "No body");
        assert!(parsed.body.is_empty());
    }

    #[test]
    fn a_title_holding_three_dashes_survives_a_round_trip() {
        let doc = Document {
            frontmatter: TestFrontmatter {
                title: "Pooling --- causes stale reads".into(),
                tags: vec!["bug".into()],
            },
            body: "The pool reuses sockets.".into(),
        };
        let serialized = serialize(&doc).unwrap();
        let parsed: Document<TestFrontmatter> = parse(&serialized).unwrap();
        assert_eq!(parsed.frontmatter, doc.frontmatter);
        assert_eq!(parsed.body, doc.body);
        let (yaml, body) = split(&serialized).unwrap();
        assert!(yaml.contains("Pooling --- causes stale reads"));
        assert_eq!(body.trim(), "The pool reuses sockets.");
    }

    #[test]
    fn a_title_that_is_only_three_dashes_survives_a_round_trip() {
        let doc = Document {
            frontmatter: TestFrontmatter {
                title: "---".into(),
                tags: vec![],
            },
            body: "body".into(),
        };
        let serialized = serialize(&doc).unwrap();
        let parsed: Document<TestFrontmatter> = parse(&serialized).unwrap();
        assert_eq!(parsed.frontmatter.title, "---");
        assert_eq!(parsed.body, "body");
    }

    #[test]
    fn a_body_that_starts_with_a_fence_keeps_it() {
        let raw = "---\ntitle: T\ntags: []\n---\n---\nbody after a rule\n";
        let parsed: Document<TestFrontmatter> = parse(raw).unwrap();
        assert_eq!(parsed.frontmatter.title, "T");
        assert_eq!(parsed.body, "---\nbody after a rule");
    }

    #[test]
    fn split_and_parse_agree_on_where_the_frontmatter_ends() {
        let raw = "---\ntitle: a --- b\ntags: [x]\n---\n\nbody\n";
        let (yaml, body) = split(raw).unwrap();
        let parsed: Document<TestFrontmatter> = parse(raw).unwrap();
        assert_eq!(yaml, "title: a --- b\ntags: [x]");
        assert_eq!(body.trim(), "body");
        assert_eq!(parsed.frontmatter.title, "a --- b");
    }

    #[test]
    fn an_unclosed_frontmatter_is_an_error() {
        let raw = "---\ntitle: T\ntags: []\n\nno closing fence\n";
        let err = parse::<TestFrontmatter>(raw).unwrap_err();
        assert!(matches!(err, Error::UnclosedFrontmatter), "{err:?}");
    }
}
