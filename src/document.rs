use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

/// A markdown document with YAML frontmatter.
#[derive(Debug)]
pub struct Document<T> {
    pub frontmatter: T,
    pub body: String,
}

/// Which delimiters wrap the frontmatter.
///
/// Two forms exist because two readers do. A tracker or a note store is
/// read through its own tool, and `---` is the convention every other
/// markdown tool knows. A documentation page is read on a git forge as
/// well, and a forge renders a leading `---` as a rule followed by the
/// raw keys. The comment form hides the same data from every markdown
/// renderer, and no reader sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fence {
    /// `---` … `---`. The default, and what `parse` and `serialize` use.
    #[default]
    Yaml,
    /// `<!-- metadata` … `-->`. A markdown renderer hides it.
    Comment,
}

impl Fence {
    /// The opening delimiter, on its own line.
    #[must_use]
    pub fn open(self) -> &'static str {
        match self {
            Fence::Yaml => "---",
            Fence::Comment => "<!-- metadata",
        }
    }

    /// The closing delimiter, on its own line.
    #[must_use]
    pub fn close(self) -> &'static str {
        match self {
            Fence::Yaml => "---",
            Fence::Comment => "-->",
        }
    }

    /// Which form a document is written in, by its opening line.
    ///
    /// The comment form is tested first. `<!-- metadata` cannot start a
    /// `---` document, so the order costs nothing and states the intent.
    #[must_use]
    pub fn detect(content: &str) -> Option<Fence> {
        let content = content.trim_start();
        [Fence::Comment, Fence::Yaml]
            .into_iter()
            .find(|&fence| strip_open(content, fence.open()).is_some())
    }
}

/// Strip an opening delimiter and the newline that must follow it.
///
/// A delimiter with text after it on the same line is not a delimiter,
/// so `----` never opens a `---` document.
fn strip_open<'a>(content: &'a str, open: &str) -> Option<&'a str> {
    let rest = content.strip_prefix(open)?;
    rest.strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
        .or_else(|| rest.is_empty().then_some(""))
}

/// Split a document into its raw frontmatter text and its body.
///
/// The frontmatter comes back as it was written, so a caller that must
/// hand it on unchanged does not re-serialize it. The skills extension
/// asks for verbatim frontmatter, and a client compares what a listing
/// gave against what a fetch gave.
pub fn split(content: &str) -> Result<(&str, &str)> {
    split_with(content, Fence::Yaml)
}

/// Split a document written in one fence form.
pub fn split_with(content: &str, fence: Fence) -> Result<(&str, &str)> {
    let (yaml, body) = split_fences(content, fence)?;
    Ok((yaml.trim_end(), body))
}

/// Find the two fences and return the YAML between them and the body after.
///
/// The closing fence is a line that holds the delimiter and nothing
/// else. A `---` inside a value is text, not a fence: a title such as
/// `Pooling --- causes stale reads` is a legal scalar, and cutting the
/// frontmatter there loses every key after it.
///
/// One helper serves both `split` and `parse`, so the verbatim text and
/// the typed frontmatter can never disagree about where a document
/// ends.
fn split_fences(content: &str, fence: Fence) -> Result<(&str, &str)> {
    let content = content.trim();
    let rest = strip_open(content, fence.open()).ok_or(Error::MissingFrontmatter)?;

    let close = fence.close();
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == close {
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
    parse_with(content, Fence::Yaml)
}

/// Parse a document written in one fence form.
pub fn parse_with<T: DeserializeOwned>(content: &str, fence: Fence) -> Result<Document<T>> {
    let (yaml, body) = split_fences(content, fence)?;
    let frontmatter: T = yaml_serde::from_str(yaml)?;

    Ok(Document {
        frontmatter,
        body: body.trim().to_string(),
    })
}

/// Parse a document in whichever form it is written, and say which.
///
/// A caller that edits a document writes it back with the fence it came
/// with. Without that, an edit silently converts the file, and every
/// page in a documentation set changes form on its first edit.
pub fn parse_any<T: DeserializeOwned>(content: &str) -> Result<(Document<T>, Fence)> {
    let fence = Fence::detect(content).ok_or(Error::MissingFrontmatter)?;
    Ok((parse_with(content, fence)?, fence))
}

/// Serialize a document back to `---`-fenced YAML frontmatter + body.
///
/// Uses `yaml_serde` for frontmatter serialization, producing canonical YAML output.
/// Tools that need specific field ordering or formatting should implement their own
/// serializer on top of this.
pub fn serialize<T: Serialize>(doc: &Document<T>) -> Result<String> {
    serialize_with(doc, Fence::Yaml)
}

/// Serialize a document in one fence form.
pub fn serialize_with<T: Serialize>(doc: &Document<T>, fence: Fence) -> Result<String> {
    let yaml = yaml_serde::to_string(&doc.frontmatter)?;
    let mut out = String::from(fence.open());
    out.push('\n');
    out.push_str(&yaml);
    out.push_str(fence.close());
    out.push('\n');
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

    #[test]
    fn parse_a_comment_fenced_document() {
        let content =
            "<!-- metadata\ntitle: \"What is Tisket?\"\ntags: []\n-->\n\n# What is Tisket?\n";
        let doc: Document<TestFrontmatter> = parse_with(content, Fence::Comment).unwrap();
        assert_eq!(doc.frontmatter.title, "What is Tisket?");
        assert_eq!(doc.body, "# What is Tisket?");
    }

    #[test]
    fn detect_names_the_form_a_document_is_written_in() {
        assert_eq!(
            Fence::detect("<!-- metadata\ntitle: t\n-->\n"),
            Some(Fence::Comment)
        );
        assert_eq!(Fence::detect("---\ntitle: t\n---\n"), Some(Fence::Yaml));
        assert_eq!(Fence::detect("no frontmatter here"), None);
    }

    #[test]
    fn a_delimiter_with_text_after_it_opens_nothing() {
        assert_eq!(Fence::detect("----\ntitle: t\n---\n"), None);
        assert_eq!(Fence::detect("<!-- metadata x\ntitle: t\n-->\n"), None);
    }

    #[test]
    fn parse_any_reports_the_fence_it_read() {
        let comment = "<!-- metadata\ntitle: T\ntags: []\n-->\n\nBody.\n";
        let (doc, fence) = parse_any::<TestFrontmatter>(comment).unwrap();
        assert_eq!(fence, Fence::Comment);
        assert_eq!(doc.frontmatter.title, "T");

        let yaml = "---\ntitle: T\ntags: []\n---\n\nBody.\n";
        let (_, fence) = parse_any::<TestFrontmatter>(yaml).unwrap();
        assert_eq!(fence, Fence::Yaml);
    }

    #[test]
    fn parse_any_refuses_a_document_with_no_frontmatter() {
        assert!(parse_any::<TestFrontmatter>("just text").is_err());
    }

    #[test]
    fn a_comment_fenced_document_survives_a_round_trip() {
        let doc = Document {
            frontmatter: TestFrontmatter {
                title: "Getting Started".into(),
                tags: vec!["docs".into()],
            },
            body: "First, run the command.".into(),
        };
        let serialized = serialize_with(&doc, Fence::Comment).unwrap();
        assert!(serialized.starts_with("<!-- metadata\n"));
        assert!(serialized.contains("\n-->\n"));
        let (parsed, fence) = parse_any::<TestFrontmatter>(&serialized).unwrap();
        assert_eq!(fence, Fence::Comment);
        assert_eq!(parsed.frontmatter, doc.frontmatter);
        assert_eq!(parsed.body, doc.body);
    }

    #[test]
    fn a_title_holding_the_comment_close_survives_a_round_trip() {
        let doc = Document {
            frontmatter: TestFrontmatter {
                title: "Arrows --> and back".into(),
                tags: vec![],
            },
            body: "body".into(),
        };
        let serialized = serialize_with(&doc, Fence::Comment).unwrap();
        let parsed: Document<TestFrontmatter> = parse_with(&serialized, Fence::Comment).unwrap();
        assert_eq!(parsed.frontmatter, doc.frontmatter);
        assert_eq!(parsed.body, "body");
    }

    #[test]
    fn an_unclosed_comment_fence_is_an_error() {
        let content = "<!-- metadata\ntitle: T\n";
        assert!(parse_with::<TestFrontmatter>(content, Fence::Comment).is_err());
    }

    #[test]
    fn each_fence_refuses_the_other_form() {
        let comment = "<!-- metadata\ntitle: T\ntags: []\n-->\n";
        assert!(parse_with::<TestFrontmatter>(comment, Fence::Yaml).is_err());
        let yaml = "---\ntitle: T\ntags: []\n---\n";
        assert!(parse_with::<TestFrontmatter>(yaml, Fence::Comment).is_err());
    }

    #[test]
    fn split_with_gives_the_comment_frontmatter_as_written() {
        let content = "<!-- metadata\nname: probe\ndescription: \"a: b\"\n-->\n\nBody here.\n";
        let (frontmatter, body) = split_with(content, Fence::Comment).unwrap();
        assert_eq!(frontmatter, "name: probe\ndescription: \"a: b\"");
        assert_eq!(body, "Body here.");
    }
}
