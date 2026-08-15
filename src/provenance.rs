use std::fmt;

use crate::error::{Error, Result};

/// A provenance marker: `origin[:qualifier] [key=value ...]`.
///
/// mdstore parses the shape and stays vocabulary-agnostic. The consumer
/// decides which origins, qualifiers, and attributes are valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub origin: String,
    pub qualifier: Option<String>,
    /// Attributes in source order. Values keep no surrounding quotes.
    pub attrs: Vec<(String, String)>,
}

impl Marker {
    /// Parse a spec string: `origin[:qualifier] [key=value ...]`.
    ///
    /// A value with spaces takes double quotes: `key="a b"`.
    pub fn parse(spec: &str) -> Result<Self> {
        let tokens = tokenize(spec)?;
        let mut iter = tokens.into_iter();
        let head = iter
            .next()
            .ok_or_else(|| Error::InvalidProvenance("empty provenance spec".into()))?;
        if head.contains('=') {
            return Err(Error::InvalidProvenance(format!(
                "spec must start with an origin, not an attribute: '{spec}'"
            )));
        }
        let (origin, qualifier) = match head.split_once(':') {
            Some((o, q)) => (o.to_string(), Some(q.to_string())),
            None => (head, None),
        };
        if origin.is_empty() {
            return Err(Error::InvalidProvenance(format!("empty origin in '{spec}'")));
        }
        if let Some(q) = &qualifier
            && q.is_empty()
        {
            return Err(Error::InvalidProvenance(format!("empty qualifier in '{spec}'")));
        }
        let mut attrs = Vec::new();
        for token in iter {
            let (key, value) = token.split_once('=').ok_or_else(|| {
                Error::InvalidProvenance(format!("expected key=value, got '{token}' in '{spec}'"))
            })?;
            if key.is_empty() {
                return Err(Error::InvalidProvenance(format!("empty attribute key in '{spec}'")));
            }
            attrs.push((key.to_string(), value.to_string()));
        }
        Ok(Marker {
            origin,
            qualifier,
            attrs,
        })
    }

    /// Get an attribute value by key.
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Set an attribute. An existing key gets the new value.
    pub fn set_attr(&mut self, key: &str, value: &str) {
        if let Some(entry) = self.attrs.iter_mut().find(|(k, _)| k == key) {
            entry.1 = value.to_string();
        } else {
            self.attrs.push((key.to_string(), value.to_string()));
        }
    }
}

impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.origin)?;
        if let Some(q) = &self.qualifier {
            write!(f, ":{q}")?;
        }
        for (k, v) in &self.attrs {
            if v.chars().any(char::is_whitespace) {
                write!(f, " {k}=\"{v}\"")?;
            } else {
                write!(f, " {k}={v}")?;
            }
        }
        Ok(())
    }
}

/// Split a spec into whitespace-separated tokens. Double quotes group a
/// value that contains spaces; the quotes do not survive into the token.
fn tokenize(spec: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in spec.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if in_quotes {
        return Err(Error::InvalidProvenance(format!("unclosed quote in '{spec}'")));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

/// A run of body text with one provenance.
///
/// `marker: None` means the text is outside every marker pair. The consumer
/// substitutes its own default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub marker: Option<Marker>,
    pub text: String,
}

const OPEN_PREFIX: &str = "prov";
const CLOSE_INNER: &str = "/prov";

/// Split a body into provenance spans.
///
/// A marker occupies its own line: `<!-- prov SPEC -->` opens a span and
/// `<!-- /prov -->` closes it. Spans are flat: a second open closes the
/// current span, and an open span with no close runs to the end of the
/// body. A close with no open span stays plain text. A `<!-- prov -->`
/// comment that does not parse is an error.
pub fn parse_spans(body: &str) -> Result<Vec<Span>> {
    let mut spans: Vec<Span> = Vec::new();
    let mut lines: Vec<&str> = Vec::new();
    let mut open: Option<Marker> = None;

    fn flush(spans: &mut Vec<Span>, marker: Option<Marker>, lines: &mut Vec<&str>) {
        // A whitespace-only unmarked span (a blank separator line between
        // markers) is kept, so render_spans restores the body exactly.
        let keep = marker.is_some() || !lines.is_empty();
        let text = lines.join("\n");
        lines.clear();
        if keep {
            spans.push(Span { marker, text });
        }
    }

    for line in body.lines() {
        match classify_line(line)? {
            MarkerLine::Open(marker) => {
                flush(&mut spans, open.take(), &mut lines);
                open = Some(marker);
            }
            MarkerLine::Close => {
                if open.is_some() {
                    flush(&mut spans, open.take(), &mut lines);
                } else {
                    lines.push(line);
                }
            }
            MarkerLine::Text => lines.push(line),
        }
    }
    flush(&mut spans, open.take(), &mut lines);
    Ok(spans)
}

enum MarkerLine {
    Open(Marker),
    Close,
    Text,
}

/// Classify one line. A line is a marker only when the whole trimmed line
/// is one `<!-- ... -->` comment whose content starts with `prov`.
fn classify_line(line: &str) -> Result<MarkerLine> {
    let trimmed = line.trim();
    let Some(inner) = trimmed
        .strip_prefix("<!--")
        .and_then(|rest| rest.strip_suffix("-->"))
    else {
        return Ok(MarkerLine::Text);
    };
    if inner.contains("-->") {
        return Ok(MarkerLine::Text);
    }
    let inner = inner.trim();
    if inner == CLOSE_INNER {
        return Ok(MarkerLine::Close);
    }
    if inner == OPEN_PREFIX {
        return Err(Error::InvalidProvenance("empty prov marker".into()));
    }
    if let Some(spec) = inner.strip_prefix(OPEN_PREFIX)
        && spec.starts_with(char::is_whitespace)
    {
        return Ok(MarkerLine::Open(Marker::parse(spec.trim())?));
    }
    Ok(MarkerLine::Text)
}

/// Every marker in a text, read the way the parser reads them.
///
/// A guard that decides whether text may be written must recognize a
/// marker exactly as the reader will. A guard with its own substring
/// scan accepts what the reader treats as a marker, which is how text
/// gets written carrying a claim the writer was supposed to refuse.
///
/// This walks whole lines through the same classifier `parse_spans`
/// uses, so the two can never drift.
pub fn markers_in(text: &str) -> Result<Vec<Marker>> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let MarkerLine::Open(marker) = classify_line(line)? {
            out.push(marker);
        }
    }
    Ok(out)
}

/// Return true if the body ends inside an open (unclosed) span.
///
/// A caller that appends text to a body needs this: text appended after
/// an open marker would silently join that span.
pub fn ends_open(body: &str) -> Result<bool> {
    let mut open = false;
    for line in body.lines() {
        match classify_line(line)? {
            MarkerLine::Open(_) => open = true,
            MarkerLine::Close => open = false,
            MarkerLine::Text => {}
        }
    }
    Ok(open)
}

/// Render spans back to a body. Marked spans get canonical marker lines.
pub fn render_spans(spans: &[Span]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for span in spans {
        match &span.marker {
            Some(marker) => {
                let mut block = format!("<!-- prov {marker} -->");
                if !span.text.is_empty() {
                    block.push('\n');
                    block.push_str(&span.text);
                }
                block.push_str("\n<!-- /prov -->");
                parts.push(block);
            }
            None => parts.push(span.text.clone()),
        }
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(spec: &str) -> Marker {
        Marker::parse(spec).unwrap()
    }

    #[test]
    fn body_with_no_markers_is_one_unmarked_span() {
        let spans = parse_spans("line one\nline two").unwrap();
        assert_eq!(
            spans,
            vec![Span {
                marker: None,
                text: "line one\nline two".into()
            }]
        );
    }

    #[test]
    fn empty_body_has_no_spans() {
        assert_eq!(parse_spans("").unwrap(), vec![]);
    }

    #[test]
    fn marked_span_in_the_middle_makes_three_spans() {
        let body = "before\n<!-- prov agent:inference -->\nguess\n<!-- /prov -->\nafter";
        let spans = parse_spans(body).unwrap();
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0], Span { marker: None, text: "before".into() });
        assert_eq!(
            spans[1],
            Span {
                marker: Some(marker("agent:inference")),
                text: "guess".into()
            }
        );
        assert_eq!(spans[2], Span { marker: None, text: "after".into() });
    }

    #[test]
    fn marker_at_start_and_end_of_body() {
        let body = "<!-- prov human:cody -->\nfirst\n<!-- /prov -->\nmiddle\n<!-- prov citation:a3f2 -->\nlast\n<!-- /prov -->";
        let spans = parse_spans(body).unwrap();
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].marker, Some(marker("human:cody")));
        assert_eq!(spans[1].marker, None);
        assert_eq!(spans[2].marker, Some(marker("citation:a3f2")));
    }

    #[test]
    fn unclosed_span_runs_to_the_end() {
        let body = "before\n<!-- prov agent:summary -->\ntail one\ntail two";
        let spans = parse_spans(body).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].marker, Some(marker("agent:summary")));
        assert_eq!(spans[1].text, "tail one\ntail two");
    }

    #[test]
    fn second_open_closes_the_first_span() {
        let body = "<!-- prov agent:summary -->\none\n<!-- prov human -->\ntwo\n<!-- /prov -->";
        let spans = parse_spans(body).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].marker, Some(marker("agent:summary")));
        assert_eq!(spans[0].text, "one");
        assert_eq!(spans[1].marker, Some(marker("human")));
        assert_eq!(spans[1].text, "two");
    }

    #[test]
    fn stray_close_stays_plain_text() {
        let body = "text\n<!-- /prov -->\nmore";
        let spans = parse_spans(body).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "text\n<!-- /prov -->\nmore");
    }

    #[test]
    fn empty_marked_span_survives() {
        let body = "<!-- prov agent:index -->\n<!-- /prov -->";
        let spans = parse_spans(body).unwrap();
        assert_eq!(
            spans,
            vec![Span {
                marker: Some(marker("agent:index")),
                text: String::new()
            }]
        );
    }

    #[test]
    fn other_html_comments_stay_text() {
        let body = "<!-- a note to self -->\n<!-- provenance-adjacent -->";
        let spans = parse_spans(body).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].marker, None);
    }

    #[test]
    fn empty_prov_marker_is_an_error() {
        assert!(parse_spans("<!-- prov -->").is_err());
    }

    #[test]
    fn attribute_without_origin_is_an_error() {
        assert!(parse_spans("<!-- prov foo=bar -->").is_err());
    }

    #[test]
    fn spec_origin_only() {
        let m = marker("human");
        assert_eq!(m.origin, "human");
        assert_eq!(m.qualifier, None);
        assert!(m.attrs.is_empty());
    }

    #[test]
    fn spec_with_qualifier() {
        let m = marker("human:cody");
        assert_eq!(m.origin, "human");
        assert_eq!(m.qualifier.as_deref(), Some("cody"));
    }

    #[test]
    fn spec_with_attrs() {
        let m = marker("agent:inference reviewed=2026-08-14 reviewer=cody");
        assert_eq!(m.attr("reviewed"), Some("2026-08-14"));
        assert_eq!(m.attr("reviewer"), Some("cody"));
    }

    #[test]
    fn attr_value_keeps_colons() {
        let m = marker("citation src=https://example.com/post?id=1");
        assert_eq!(m.origin, "citation");
        assert_eq!(m.attr("src"), Some("https://example.com/post?id=1"));
    }

    #[test]
    fn quoted_attr_value_keeps_spaces() {
        let m = marker("citation:ddia title=\"Designing Data-Intensive Applications\"");
        assert_eq!(m.attr("title"), Some("Designing Data-Intensive Applications"));
    }

    #[test]
    fn unclosed_quote_is_an_error() {
        assert!(Marker::parse("citation title=\"oops").is_err());
    }

    #[test]
    fn empty_origin_or_qualifier_is_an_error() {
        assert!(Marker::parse("").is_err());
        assert!(Marker::parse(":inference").is_err());
        assert!(Marker::parse("agent:").is_err());
    }

    #[test]
    fn bare_token_after_origin_is_an_error() {
        assert!(Marker::parse("agent inference").is_err());
    }

    #[test]
    fn marker_display_roundtrips() {
        for spec in [
            "human",
            "human:cody",
            "agent:inference reviewed=2026-08-14",
            "citation:ddia title=\"a b\" p=212",
        ] {
            let m = marker(spec);
            assert_eq!(Marker::parse(&m.to_string()).unwrap(), m);
        }
    }

    #[test]
    fn set_attr_updates_in_place() {
        let mut m = marker("agent:summary");
        m.set_attr("reviewed", "2026-08-14");
        m.set_attr("reviewed", "2026-08-15");
        assert_eq!(m.attr("reviewed"), Some("2026-08-15"));
        assert_eq!(m.attrs.len(), 1);
    }

    #[test]
    fn blank_lines_between_markers_round_trip() {
        let body = "<!-- prov agent:summary -->\none\n<!-- /prov -->\n\n<!-- prov human -->\ntwo\n<!-- /prov -->";
        let spans = parse_spans(body).unwrap();
        assert_eq!(spans.len(), 3, "the blank separator is its own span");
        assert_eq!(spans[1], Span { marker: None, text: String::new() });
        assert_eq!(render_spans(&spans), body);
    }

    #[test]
    fn render_roundtrips_through_parse() {
        let body = "before\n<!-- prov agent:inference -->\nguess\n<!-- /prov -->\nafter";
        let spans = parse_spans(body).unwrap();
        let rendered = render_spans(&spans);
        assert_eq!(parse_spans(&rendered).unwrap(), spans);
        assert_eq!(rendered, body);
    }

    #[test]
    fn render_normalizes_an_unclosed_span() {
        let spans = parse_spans("<!-- prov agent:summary -->\ntail").unwrap();
        let rendered = render_spans(&spans);
        assert_eq!(rendered, "<!-- prov agent:summary -->\ntail\n<!-- /prov -->");
    }

    #[test]
    fn ends_open_detects_an_unclosed_span() {
        assert!(ends_open("<!-- prov agent:summary -->\ntail").unwrap());
        assert!(!ends_open("<!-- prov agent:summary -->\ntail\n<!-- /prov -->").unwrap());
        assert!(!ends_open("plain text").unwrap());
        assert!(!ends_open("").unwrap());
        // A stray close with no open does not make the body "open".
        assert!(!ends_open("text\n<!-- /prov -->").unwrap());
    }

    #[test]
    fn marker_inside_code_fence_is_parsed_v1_limitation() {
        // Pins the v1 behavior: fenced code blocks are not exempt.
        let body = "```\n<!-- prov human -->\n```";
        let spans = parse_spans(body).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].marker, Some(marker("human")));
    }
}
