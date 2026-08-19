//! A store, rendered as a book a person can read.
//!
//! mdbook renders an ordered tree of chapters. A store is a graph. This
//! module maps one onto the other, and it makes one commitment that the
//! rest follows from: a document's page path is its id, flat. Grouping
//! changes the sidebar and never the path, so a link a person saved
//! keeps working when the grouping changes.
//!
//! What this module does: the tree, the link rewriting, the frontmatter
//! table, and the generated chapters that show the graph.
//!
//! What a consumer does: say what a document is called, what it holds,
//! and which group it belongs under. [`Chaptered`] is that seam.
//! `Snapshot` knows ids and references; only the consumer knows that an
//! issue has a status or a note has a tag.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use mdbook_core::book::{Book, BookItem, Chapter};

use crate::snapshot::{DocId, DocumentSource, Snapshot};

/// What a document must tell the mapping about itself.
pub trait Chaptered {
    /// The heading a reader sees, and the sidebar entry.
    fn title(&self) -> String;

    /// The markdown body, without frontmatter.
    fn body(&self) -> &str;

    /// Frontmatter as ordered rows, rendered as a table at the head of
    /// the page. Empty for a document whose frontmatter tells a reader
    /// nothing.
    fn rows(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// The group this document sits under in the sidebar, if any. A
    /// tracker groups by status, a note store by tag, a library by
    /// name. `None` puts the document at the top level.
    fn group(&self) -> Option<String> {
        None
    }

    /// A sub-page hanging off this one, as a title and a body. A tisket
    /// issue carries its scratch this way.
    fn sub_pages(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// The page path of a document: its id, flat, always.
#[must_use]
pub fn page_path(id: &str) -> String {
    format!("{id}.md")
}

/// The page path of a document in a declared store, under the alias.
///
/// A foreign id lives in its own directory, so it cannot collide with a
/// local one.
#[must_use]
pub fn foreign_page_path(qualified: &str) -> String {
    match qualified.split_once(':') {
        Some((alias, id)) => format!("{alias}/{id}.md"),
        None => page_path(qualified),
    }
}

/// Rewrite `[[id]]` and `[[alias:id]]` into links mdbook resolves.
///
/// mdbook turns a relative `.md` link into the rendered `.html`, so the
/// rewrite targets the markdown path rather than the output path.
#[must_use]
pub fn rewrite_links(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        let (before, from_open) = rest.split_at(start);
        out.push_str(before);
        let Some(end) = from_open.find("]]") else {
            // An unclosed marker is text, not a link. Emit it and stop.
            out.push_str(from_open);
            return out;
        };
        let target = &from_open[2..end];
        if target.is_empty() || target.contains(char::is_whitespace) {
            // Not a reference. Leave the source alone rather than
            // guessing at what a person meant.
            out.push_str(&from_open[..end + 2]);
        } else {
            let path = foreign_page_path(target);
            let _ = write!(out, "[{target}]({path})");
        }
        rest = &from_open[end + 2..];
    }
    out.push_str(rest);
    out
}

/// The frontmatter table, or an empty string when there are no rows.
fn rows_table(rows: &[(String, String)]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut table = String::from("| | |\n|---|---|\n");
    for (key, value) in rows {
        // A pipe in a value would end the cell, so it is escaped.
        let value = value.replace('|', "\\|");
        let _ = writeln!(table, "| {key} | {value} |");
    }
    table.push('\n');
    table
}

/// One chapter, with its frontmatter table, its body and its sub-pages.
fn chapter_of<D: Chaptered>(id: &str, doc: &D) -> Chapter {
    let path = page_path(id);
    let content = format!(
        "# {}\n\n{}{}\n",
        doc.title(),
        rows_table(&doc.rows()),
        rewrite_links(doc.body())
    );
    let sub_items = doc
        .sub_pages()
        .into_iter()
        .map(|(title, body)| {
            BookItem::Chapter(Chapter {
                name: title.clone(),
                content: format!("# {title}\n\n{}\n", rewrite_links(&body)),
                number: None,
                sub_items: Vec::new(),
                path: Some(format!("{id}-{}.md", crate::slug::slugify(&title)).into()),
                source_path: None,
                parent_names: vec![doc.title()],
            })
        })
        .collect();
    Chapter {
        name: doc.title(),
        content,
        number: None,
        sub_items,
        path: Some(path.into()),
        source_path: None,
        parent_names: Vec::new(),
    }
}

/// Build a book from one store's documents, grouped as the documents ask.
///
/// Foreign documents are left out. A declared store is another store's
/// content, and a reader of this one follows a link to reach it.
pub fn to_book<S>(snapshot: &Snapshot<S>) -> Book
where
    S: DocumentSource,
    S::Doc: Chaptered,
{
    let mut grouped: BTreeMap<String, Vec<(DocId, &S::Doc)>> = BTreeMap::new();
    let mut ungrouped: Vec<(DocId, &S::Doc)> = Vec::new();
    for (id, entry) in snapshot.documents() {
        if snapshot.is_foreign(id) {
            continue;
        }
        match entry.doc.group() {
            Some(group) => grouped.entry(group).or_default().push((id, &entry.doc)),
            None => ungrouped.push((id, &entry.doc)),
        }
    }

    let mut items = Vec::new();
    for (id, doc) in ungrouped {
        items.push(BookItem::Chapter(chapter_of(entry_id(snapshot, id), doc)));
    }
    for (group, docs) in grouped {
        items.push(BookItem::PartTitle(group));
        for (id, doc) in docs {
            items.push(BookItem::Chapter(chapter_of(entry_id(snapshot, id), doc)));
        }
    }
    Book::new_with_items(items)
}

/// The id a document is filed under, as written in the store.
fn entry_id<S: DocumentSource>(snapshot: &Snapshot<S>, id: DocId) -> &str {
    snapshot
        .get(id)
        .map(|entry| entry.id.as_str())
        .unwrap_or_default()
}

/// Render a book to HTML in `destination`.
///
/// The caller supplies documents and a title, and nothing else. mdbook
/// needs a source directory to exist even when every chapter is
/// synthetic, and it fails with a bare `No such file or directory` when
/// one is absent, so a scratch directory is made and removed here rather
/// than becoming a caller's problem.
///
/// Search is on. It needs the `search` feature of `mdbook-html`, which
/// this crate turns on for the `book` feature, because a store nobody can
/// search is a store nobody can read.
#[cfg(feature = "book")]
pub fn render_html(
    book: Book,
    title: &str,
    destination: &std::path::Path,
) -> crate::error::Result<()> {
    use mdbook_renderer::{RenderContext, Renderer};

    let scratch = std::env::temp_dir().join(format!(
        "mdstore-book-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let src = scratch.join("src");
    std::fs::create_dir_all(&src).map_err(|source| crate::error::Error::StorePath {
        rel: "src".to_string(),
        root: scratch.display().to_string(),
        source,
    })?;

    let mut config = mdbook_core::config::Config::default();
    config.book.title = Some(title.to_string());

    let ctx = RenderContext::new(&scratch, book, config, destination);
    let rendered = mdbook_html::HtmlHandlebars::new()
        .render(&ctx)
        .map_err(|e| crate::error::Error::InvalidStore(format!("cannot render the book: {e}")));

    // The scratch directory is removed whether the render worked or not.
    let _ = std::fs::remove_dir_all(&scratch);
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Doc {
        title: &'static str,
        body: &'static str,
        rows: Vec<(&'static str, &'static str)>,
        group: Option<&'static str>,
        sub: Vec<(&'static str, &'static str)>,
    }

    impl Chaptered for Doc {
        fn title(&self) -> String {
            self.title.to_string()
        }
        fn body(&self) -> &str {
            self.body
        }
        fn rows(&self) -> Vec<(String, String)> {
            self.rows
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect()
        }
        fn group(&self) -> Option<String> {
            self.group.map(str::to_string)
        }
        fn sub_pages(&self) -> Vec<(String, String)> {
            self.sub
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect()
        }
    }

    fn doc(body: &'static str) -> Doc {
        Doc {
            title: "A title",
            body,
            rows: Vec::new(),
            group: None,
            sub: Vec::new(),
        }
    }

    #[test]
    fn a_page_path_is_the_id_and_a_foreign_one_sits_under_its_alias() {
        assert_eq!(page_path("5jls"), "5jls.md");
        assert_eq!(foreign_page_path("handbook:5jls"), "handbook/5jls.md");
        // No alias means a local document, whatever the caller passed.
        assert_eq!(foreign_page_path("5jls"), "5jls.md");
    }

    #[test]
    fn a_reference_becomes_a_link_that_mdbook_resolves() {
        assert_eq!(rewrite_links("see [[5jls]]"), "see [5jls](5jls.md)");
        assert_eq!(
            rewrite_links("see [[handbook:5jls]] and [[78sn]]"),
            "see [handbook:5jls](handbook/5jls.md) and [78sn](78sn.md)"
        );
    }

    #[test]
    fn text_that_is_not_a_reference_is_left_alone() {
        // Each of these would be corrupted by a careless rewrite.
        for text in [
            "an unclosed [[marker",
            "empty [[]] brackets",
            "a [[two words]] phrase",
            "no markers at all",
        ] {
            assert_eq!(rewrite_links(text), text, "{text:?} was rewritten");
        }
    }

    #[test]
    fn a_pipe_in_a_value_cannot_end_the_cell() {
        let rows = vec![("labels".to_string(), "a|b".to_string())];
        let table = rows_table(&rows);
        assert!(table.contains(r"a\|b"), "{table}");
        // No rows means no table, not an empty one.
        assert_eq!(rows_table(&[]), "");
    }

    #[test]
    fn a_chapter_carries_the_table_the_body_and_its_sub_page() {
        let d = Doc {
            title: "An issue",
            body: "Links to [[78sn]].",
            rows: vec![("status", "todo")],
            group: Some("todo"),
            sub: vec![("Scratch", "Working notes, see [[5jls]].")],
        };
        let ch = chapter_of("2x7j", &d);
        assert_eq!(ch.path.as_deref(), Some(std::path::Path::new("2x7j.md")));
        assert!(ch.source_path.is_none(), "the page has no file on disk");
        assert!(ch.content.contains("# An issue"));
        assert!(ch.content.contains("| status | todo |"));
        assert!(ch.content.contains("[78sn](78sn.md)"));
        assert_eq!(ch.sub_items.len(), 1);
        let BookItem::Chapter(sub) = &ch.sub_items[0] else {
            panic!("the sub-page is a chapter");
        };
        assert_eq!(
            sub.path.as_deref(),
            Some(std::path::Path::new("2x7j-scratch.md"))
        );
        assert!(
            sub.content.contains("[5jls](5jls.md)"),
            "the sub-page rewrites too"
        );
        assert_eq!(sub.parent_names, vec!["An issue".to_string()]);
    }

    #[test]
    fn a_body_with_no_frontmatter_gets_no_table() {
        let ch = chapter_of("5jls", &doc("Just prose."));
        assert!(!ch.content.contains("|---|"), "{}", ch.content);
    }
}
