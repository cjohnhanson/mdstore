use serde::de::DeserializeOwned;
use serde::Serialize;

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
    let content = content.trim();
    if !content.starts_with("---") {
        return Err(Error::MissingFrontmatter);
    }
    let after_first = &content[3..].trim_start_matches('\n');
    let end = after_first.find("---").ok_or(Error::UnclosedFrontmatter)?;
    Ok((
        after_first[..end].trim_end(),
        after_first[end + 3..].trim_start_matches('\n'),
    ))
}

/// Parse a `---`-fenced YAML frontmatter document into a typed frontmatter and body.
pub fn parse<T: DeserializeOwned>(content: &str) -> Result<Document<T>> {
    let content = content.trim();
    if !content.starts_with("---") {
        return Err(Error::MissingFrontmatter);
    }

    let after_first = &content[3..].trim_start_matches('\n');
    let end = after_first
        .find("---")
        .ok_or(Error::UnclosedFrontmatter)?;

    let yaml = &after_first[..end];
    let frontmatter: T = yaml_serde::from_str(yaml)?;
    let body = after_first[end + 3..].trim().to_string();

    Ok(Document { frontmatter, body })
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
}
