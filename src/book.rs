//! A store, rendered as a book a person can read.
//!
//! mdbook renders an ordered tree of chapters. A store is a graph. This
//! module maps one onto the other, and it makes one commitment that the
//! rest follows from: a document's page path is its id, flat. Grouping
//! changes the sidebar and never the path, so a link a person saved
//! keeps working when the grouping changes.
//!
//! What this module does: the tree, the link rewriting, the frontmatter
//! table, and the render.
//!
//! What a consumer does: say what a document is called, what it holds,
//! and which group it belongs under. [`Chaptered`] is that seam.
//! `Snapshot` knows ids and references; only the consumer knows that an
//! issue has a status or a note has a tag.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use mdbook_core::book::{Book, BookItem, Chapter};
use rand::RngExt;

use crate::error::{Error, Result};
use crate::snapshot::{DocumentSource, Snapshot};

/// The file this module writes into a destination it owns.
///
/// The render empties its destination, so it must be able to tell a
/// directory it wrote from a directory a person cares about.
const MARKER: &str = ".mdstore-book";

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

/// True when `id` can become a page path and a link destination.
///
/// [`crate::store::is_plain_stem`] carries most of it, because an id
/// becoming a file path is the reason that predicate exists. Three more
/// rules apply here, and each one is a link that would otherwise break:
/// a space cannot appear in a bare markdown destination, a `:` reads as
/// a store alias, and a parenthesis ends a destination.
#[must_use]
pub fn is_page_id(id: &str) -> bool {
    crate::store::is_plain_stem(id)
        && !id.contains(' ')
        && !id.contains(':')
        && !id.contains('(')
        && !id.contains(')')
}

/// The page path of a document: its id, flat, always.
///
/// `None` when the id cannot become one, rather than a path that leaves
/// the destination or a link nothing resolves.
#[must_use]
pub fn page_path(id: &str) -> Option<String> {
    is_page_id(id).then(|| format!("{id}.md"))
}

/// The page path of a document in a declared store, under its alias.
///
/// A foreign id lives in its own directory, so it cannot collide with a
/// local one. Both halves are checked, so `../etc:passwd` is refused
/// rather than written outside the destination.
#[must_use]
pub fn foreign_page_path(qualified: &str) -> Option<String> {
    match qualified.split_once(':') {
        Some((alias, id)) => {
            (is_page_id(alias) && is_page_id(id)).then(|| format!("{alias}/{id}.md"))
        }
        None => page_path(qualified),
    }
}

/// The path of a sub-page, under a directory named for its document.
///
/// A sub-page lives at `<id>/<n>-<slug>.md`, so it cannot take the path
/// of a document whose id reads like `<id>-<slug>`. The index keeps two
/// sub-pages with one slug apart, which `Scratch!` and `Scratch?` share.
#[must_use]
fn sub_page_path(id: &str, index: usize, title: &str) -> Option<String> {
    if !is_page_id(id) {
        return None;
    }
    let slug = crate::slug::slugify(title);
    let slug = if slug.is_empty() {
        "page".to_string()
    } else {
        slug
    };
    Some(format!("{id}/{}-{slug}.md", index + 1))
}

/// Rewrite `[[id]]` and `[[alias:id]]` into links mdbook resolves.
///
/// mdbook turns a relative `.md` link into the rendered `.html`, so the
/// rewrite targets the markdown path rather than the output path.
///
/// Code is left alone. A fenced block and an inline span are copied
/// through untouched, because these tools document `[[id]]` syntax and a
/// rewritten sample stops being a sample. A reference this module cannot
/// turn into a path is also left alone, so prose that merely looks like
/// a reference survives.
#[must_use]
pub fn rewrite_links(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut fence: Option<&str> = None;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(marker) = fence {
            out.push_str(line);
            if trimmed.starts_with(marker) {
                fence = None;
            }
            continue;
        }
        if trimmed.starts_with("```") {
            fence = Some("```");
            out.push_str(line);
            continue;
        }
        if trimmed.starts_with("~~~") {
            fence = Some("~~~");
            out.push_str(line);
            continue;
        }
        rewrite_line(line, &mut out);
    }
    out
}

/// One line, outside a fence. An inline code span passes through.
fn rewrite_line(line: &str, out: &mut String) {
    let mut rest = line;
    let mut in_code = false;
    while let Some(next) = rest.find(['`', '[', ']']) {
        let (before, from) = rest.split_at(next);
        out.push_str(before);
        if let Some(after) = from.strip_prefix('`') {
            in_code = !in_code;
            out.push('`');
            rest = after;
            continue;
        }
        // A link destination is copied through. A reference inside one
        // once became nested brackets that no parser reads as a link.
        if let Some(after) = from.strip_prefix("](") {
            let line_end = after.find('\n').unwrap_or(after.len());
            match after[..line_end].find(')') {
                Some(close) => {
                    out.push_str(&from[..2 + close + 1]);
                    rest = &after[close + 1..];
                }
                None => {
                    out.push_str("](");
                    rest = after;
                }
            }
            continue;
        }
        if let Some(after) = from.strip_prefix(']') {
            out.push(']');
            rest = after;
            continue;
        }
        if in_code || !from.starts_with("[[") {
            // Inside a code span, or one bracket. Emit it and advance by
            // a byte, so a single `[` never starts a rewrite.
            out.push('[');
            rest = &from[1..];
            continue;
        }
        // A reference closes on its own line or it is not one.
        let line_end = from.find('\n').unwrap_or(from.len());
        match from[..line_end].find("]]") {
            Some(end) => {
                let target = &from[2..end];
                match foreign_page_path(target) {
                    // Angle brackets, so a destination holding an odd
                    // character still parses as one destination.
                    Some(path) => {
                        let _ = write!(out, "[{target}](<{path}>)");
                    }
                    None => out.push_str(&from[..end + 2]),
                }
                rest = &from[end + 2..];
            }
            None => {
                out.push_str("[[");
                rest = &from[2..];
            }
        }
    }
    out.push_str(rest);
}

/// A table cell. A pipe ends a cell and a newline ends the row.
fn cell(text: &str) -> String {
    text.replace('\\', r"\\")
        .replace('|', r"\|")
        .replace(['\n', '\r'], " ")
}

/// The frontmatter table, or an empty string when there are no rows.
fn rows_table(rows: &[(String, String)]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut table = String::from("| | |\n|---|---|\n");
    for (key, value) in rows {
        let _ = writeln!(table, "| {} | {} |", cell(key), cell(value));
    }
    table.push('\n');
    table
}

/// One chapter, with its frontmatter table, its body and its sub-pages.
///
/// `None` when the id cannot become a page path, so a document with an
/// unusable id is left out rather than written somewhere unintended.
fn chapter_of<D: Chaptered>(id: &str, doc: &D) -> Option<Chapter> {
    let path = page_path(id)?;
    let content = format!(
        "# {}\n\n{}{}\n",
        doc.title(),
        rows_table(&doc.rows()),
        rewrite_links(doc.body())
    );
    let sub_items = doc
        .sub_pages()
        .into_iter()
        .enumerate()
        .filter_map(|(index, (title, body))| {
            let sub_path = sub_page_path(id, index, &title)?;
            Some(BookItem::Chapter(Chapter {
                name: title.clone(),
                content: format!("# {title}\n\n{}\n", rewrite_links(&body)),
                number: None,
                sub_items: Vec::new(),
                path: Some(sub_path.into()),
                source_path: None,
                parent_names: vec![doc.title()],
            }))
        })
        .collect();
    Some(Chapter {
        name: doc.title(),
        content,
        number: None,
        sub_items,
        path: Some(path.into()),
        source_path: None,
        parent_names: Vec::new(),
    })
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
    let mut grouped: BTreeMap<String, Vec<Chapter>> = BTreeMap::new();
    let mut ungrouped: Vec<Chapter> = Vec::new();
    for (id, entry) in snapshot.documents() {
        if snapshot.is_foreign(id) {
            continue;
        }
        let Some(chapter) = chapter_of(&entry.id, &entry.doc) else {
            continue;
        };
        match entry.doc.group() {
            Some(group) => grouped.entry(group).or_default().push(chapter),
            None => ungrouped.push(chapter),
        }
    }

    let mut items: Vec<BookItem> = ungrouped.into_iter().map(BookItem::Chapter).collect();
    for (group, chapters) in grouped {
        items.push(BookItem::PartTitle(group));
        items.extend(chapters.into_iter().map(BookItem::Chapter));
    }
    Book::new_with_items(items)
}

/// True when this module may empty `destination`.
///
/// Absent or empty is safe. A directory carrying the marker was written
/// by a previous render, so a re-render may replace it. Anything else
/// belongs to a person and is refused.
fn may_write(destination: &Path) -> Result<bool> {
    if !destination.exists() {
        return Ok(true);
    }
    if destination.join(MARKER).is_file() {
        return Ok(true);
    }
    let mut entries = std::fs::read_dir(destination).map_err(|source| Error::StorePath {
        rel: ".".to_string(),
        root: destination.display().to_string(),
        source,
    })?;
    Ok(entries.next().is_none())
}

/// A scratch directory nothing can predict.
///
/// A predictable path in a shared temp directory lets another process
/// pre-create it and plant files, which the render then copies into the
/// published site. Only `.md` is excluded from that copy, so a planted
/// script reaches a reader.
fn scratch_dir_in(base: &Path) -> std::path::PathBuf {
    let mut rng = rand::rng();
    let suffix: String = (0..16)
        .map(|_| char::from(b'a' + rng.random_range(0..26u8)))
        .collect();
    base.join(format!("mdstore-book-{suffix}"))
}

/// Render a book to HTML in `destination`.
///
/// The destination is emptied. mdbook removes the content of its output
/// directory before it writes, so this refuses a destination holding
/// anything except the marker a previous render left. A rendered book is
/// disposable output; a person's directory is not.
///
/// A scratch directory is made and removed here. mdbook needs a source
/// directory to exist even when every chapter is synthetic, and it fails
/// with a bare `No such file or directory` when one is absent. The name
/// is random, because a predictable path in a shared temp directory lets
/// another process plant files that the render copies into the site.
///
/// Search is on. It needs the `search` feature of `mdbook-html`, which
/// this crate turns on for the `book` feature, because a store nobody
/// can search is a store nobody can read.
pub fn render_html(book: Book, title: &str, destination: &Path) -> Result<()> {
    render_html_in(&std::env::temp_dir(), book, title, destination)
}

/// `render_html`, with the scratch base named.
///
/// The base is a parameter so a test can watch the scratch directory
/// appear and go without scanning a shared temp directory, which races
/// every other test that renders.
fn render_html_in(scratch_base: &Path, book: Book, title: &str, destination: &Path) -> Result<()> {
    use mdbook_renderer::{RenderContext, Renderer};

    if book.items.is_empty() {
        return Err(Error::InvalidStore(
            "the store holds no local document, so the book would have no page".to_string(),
        ));
    }
    if !may_write(destination)? {
        return Err(Error::InvalidStore(format!(
            "{} is not empty and no render wrote it. A render empties its destination, so \
             it refuses this one",
            destination.display()
        )));
    }

    let scratch = scratch_dir_in(scratch_base);
    let src = scratch.join("src");
    std::fs::create_dir_all(&src).map_err(|source| Error::StorePath {
        rel: "src".to_string(),
        root: scratch.display().to_string(),
        source,
    })?;

    let mut config = mdbook_core::config::Config::default();
    config.book.title = Some(title.to_string());

    let ctx = RenderContext::new(&scratch, book, config, destination);
    let rendered = mdbook_html::HtmlHandlebars::new()
        .render(&ctx)
        .map_err(|e| Error::InvalidStore(format!("cannot render the book: {e}")));

    // The scratch directory goes whether the render worked or not. The
    // render's own error is kept; only the removal's is dropped.
    let _ = std::fs::remove_dir_all(&scratch);
    rendered?;

    std::fs::write(destination.join(MARKER), "written by mdstore\n").map_err(|source| {
        Error::StorePath {
            rel: MARKER.to_string(),
            root: destination.display().to_string(),
            source,
        }
    })
}

#[cfg(test)]
mod tests;
