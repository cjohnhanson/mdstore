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

fn scratch(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "mdstore-bk-test-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn a_page_path_is_the_id_and_a_foreign_one_sits_under_its_alias() {
    assert_eq!(page_path("5jls").as_deref(), Some("5jls.md"));
    assert_eq!(
        foreign_page_path("handbook:5jls").as_deref(),
        Some("handbook/5jls.md")
    );
    assert_eq!(foreign_page_path("5jls").as_deref(), Some("5jls.md"));
}

#[test]
fn an_id_that_cannot_be_a_path_has_no_page() {
    // Each of these becomes a file path, so each is refused. The `..`
    // and absolute cases would leave the destination; the rest would
    // build a link nothing resolves.
    for bad in [
        "",
        ".",
        "..",
        "../etc",
        "/etc",
        "a/b",
        "a\\b",
        ".hidden",
        "a b",
        "with:colon",
        "a)b",
        "a(b",
        "with\0nul",
    ] {
        assert!(page_path(bad).is_none(), "{bad:?} produced a page");
    }
    // A foreign reference is refused when either half is unusable.
    for bad in [":x", "a:", "../etc:passwd", "a:b:c"] {
        assert!(
            foreign_page_path(bad).is_none(),
            "{bad:?} produced a page path"
        );
    }
}

#[test]
fn a_reference_becomes_a_link_that_mdbook_resolves() {
    assert_eq!(rewrite_links("see [[5jls]]"), "see [5jls](<5jls.md>)");
    assert_eq!(
        rewrite_links("see [[handbook:5jls]] and [[78sn]]"),
        "see [handbook:5jls](<handbook/5jls.md>) and [78sn](<78sn.md>)"
    );
}

#[test]
fn text_that_is_not_a_reference_is_left_alone() {
    for text in [
        "an unclosed [[marker",
        "empty [[]] brackets",
        "a [[two words]] phrase",
        "no markers at all",
        "one [bracket] only",
        "a [[../escape]] attempt",
        "a [[a)b]] paren",
    ] {
        assert_eq!(rewrite_links(text), text, "{text:?} was rewritten");
    }
}

#[test]
fn code_is_not_rewritten() {
    // These tools document the `[[id]]` syntax, so a sample showing it
    // must survive. A rewritten sample stops being a sample.
    let fenced = "before\n\n```\nsee [[5jls]]\n```\n\nafter\n";
    assert_eq!(
        rewrite_links(fenced),
        fenced,
        "a fenced block was rewritten"
    );

    let tilde = "~~~markdown\nlink with [[5jls]]\n~~~\n";
    assert_eq!(rewrite_links(tilde), tilde, "a tilde fence was rewritten");

    let inline = "type `[[5jls]]` to link\n";
    assert_eq!(
        rewrite_links(inline),
        inline,
        "an inline span was rewritten"
    );

    // A reference after a closed fence is still a reference.
    let mixed = "```\n[[5jls]]\n```\nthen [[78sn]]\n";
    let out = rewrite_links(mixed);
    assert!(out.contains("```\n[[5jls]]\n```"), "{out}");
    assert!(out.contains("then [78sn](<78sn.md>)"), "{out}");
}

#[test]
fn an_existing_link_is_not_corrupted() {
    // A reference inside a link destination once produced nested
    // brackets that no parser reads as a link.
    let text = "[a](dir/[[5jls]].md)";
    let out = rewrite_links(text);
    assert!(
        !out.contains("](<5jls.md>).md)"),
        "the destination was rewritten: {out}"
    );
}

#[test]
fn an_unclosed_marker_does_not_swallow_a_later_reference() {
    let out = rewrite_links("an unclosed [[marker\nand [[5jls]] after\n");
    assert!(out.starts_with("an unclosed [[marker\n"), "{out}");
    assert!(out.contains("and [5jls](<5jls.md>) after"), "{out}");
}

#[test]
fn a_cell_cannot_end_its_own_row() {
    let rows = vec![
        ("labels".to_string(), "a|b".to_string()),
        ("note".to_string(), "line one\nline two".to_string()),
        ("a|b".to_string(), "v".to_string()),
        ("pre".to_string(), r"already \| escaped".to_string()),
    ];
    let table = rows_table(&rows);
    // Every row holds exactly two unescaped separators plus the edges.
    for line in table.lines().skip(2).filter(|l| !l.is_empty()) {
        let unescaped = line
            .replace(r"\\", "")
            .replace(r"\|", "")
            .matches('|')
            .count();
        assert_eq!(unescaped, 3, "row {line:?} holds {unescaped} separators");
    }
    assert!(!table.contains("line one\nline two"), "a newline survived");
    assert_eq!(rows_table(&[]), "", "no rows means no table");
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
    let ch = chapter_of("2x7j", &d).expect("a plain id has a page");
    assert_eq!(ch.path.as_deref(), Some(std::path::Path::new("2x7j.md")));
    assert!(ch.source_path.is_none(), "the page has no file on disk");
    assert!(ch.content.contains("# An issue"));
    assert!(ch.content.contains("| status | todo |"));
    assert!(ch.content.contains("[78sn](<78sn.md>)"));
    let BookItem::Chapter(sub) = &ch.sub_items[0] else {
        panic!("the sub-page is a chapter");
    };
    assert_eq!(
        sub.path.as_deref(),
        Some(std::path::Path::new("2x7j/1-scratch.md"))
    );
    assert!(sub.content.contains("[5jls](<5jls.md>)"));
    assert_eq!(sub.parent_names, vec!["An issue".to_string()]);
}

#[test]
fn a_sub_page_cannot_take_another_page_s_path() {
    // Two sub-pages whose titles slug the same once shared one path, and
    // the second write won.
    let d = Doc {
        title: "An issue",
        body: "",
        rows: Vec::new(),
        group: None,
        sub: vec![("Scratch!", "one"), ("Scratch?", "two"), ("***", "three")],
    };
    let ch = chapter_of("2x7j", &d).unwrap();
    let paths: Vec<String> = ch
        .sub_items
        .iter()
        .map(|item| {
            let BookItem::Chapter(c) = item else {
                panic!("a chapter")
            };
            c.path.as_ref().unwrap().display().to_string()
        })
        .collect();
    let mut unique = paths.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        paths.len(),
        "two sub-pages share a path: {paths:?}"
    );
    // A sub-page sits under its document, so it cannot collide with a
    // document whose id reads like `<id>-<slug>`.
    for p in &paths {
        assert!(p.starts_with("2x7j/"), "{p} is not under its document");
    }
    // An empty slug still names a page.
    assert!(paths.iter().any(|p| p.ends_with("3-page.md")), "{paths:?}");
}

#[test]
fn a_body_with_no_frontmatter_gets_no_table() {
    let ch = chapter_of("5jls", &doc("Just prose.")).unwrap();
    assert!(!ch.content.contains("|---|"), "{}", ch.content);
}

#[test]
fn a_render_refuses_a_destination_it_did_not_write() {
    let dir = scratch("refuse");
    std::fs::create_dir_all(dir.join("precious")).unwrap();
    std::fs::write(dir.join("README.md"), "a person's file").unwrap();
    std::fs::write(dir.join("precious/notes.txt"), "keep me").unwrap();

    let book = Book::new_with_items(vec![BookItem::Chapter(
        chapter_of("5jls", &doc("A page.")).unwrap(),
    )]);
    let err = render_html(book, "A book", &dir).expect_err("a full directory is refused");
    assert!(err.to_string().contains("empties its destination"), "{err}");

    // Nothing was removed.
    assert!(dir.join("README.md").is_file(), "the render deleted a file");
    assert!(
        dir.join("precious/notes.txt").is_file(),
        "the render deleted a directory"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_render_writes_a_marker_and_then_accepts_its_own_destination() {
    let dir = scratch("marker");
    let book = || {
        Book::new_with_items(vec![BookItem::Chapter(
            chapter_of("5jls", &doc("A page.")).unwrap(),
        )])
    };
    render_html(book(), "A book", &dir).expect("an absent destination is fine");
    assert!(dir.join(MARKER).is_file(), "no marker was written");
    assert!(dir.join("5jls.html").is_file(), "no page was written");
    // A second render replaces its own output rather than refusing.
    render_html(book(), "A book", &dir).expect("its own destination is fine");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_book_is_an_error_rather_than_a_site_with_no_page() {
    let dir = scratch("empty");
    let err = render_html(Book::new_with_items(Vec::new()), "A book", &dir)
        .expect_err("an empty book is refused");
    assert!(err.to_string().contains("no local document"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_scratch_name_is_not_predictable() {
    // A predictable path in a shared temp directory lets another process
    // plant files that the render copies into the site.
    let base = std::path::Path::new("/tmp");
    let a = scratch_dir_in(base);
    let b = scratch_dir_in(base);
    assert_ne!(a, b, "two scratch names matched");
    for path in [&a, &b] {
        let name = path.file_name().unwrap().to_string_lossy();
        let suffix = name.strip_prefix("mdstore-book-").expect("the prefix");
        assert_eq!(suffix.len(), 16, "{name} carries a short suffix");
        assert!(
            suffix.bytes().all(|b| b.is_ascii_lowercase()),
            "{name} holds a byte outside the alphabet"
        );
    }
}

#[test]
fn a_render_leaves_no_scratch_directory_behind() {
    // The base is this test's own, so the count cannot race another
    // test's render.
    let base = scratch("base");
    std::fs::create_dir_all(&base).unwrap();
    let dest = scratch("residue-out");
    let book = Book::new_with_items(vec![BookItem::Chapter(
        chapter_of("5jls", &doc("A page.")).unwrap(),
    )]);
    render_html_in(&base, book, "A book", &dest).unwrap();
    let left: Vec<String> = std::fs::read_dir(&base)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(left.is_empty(), "a scratch directory survived: {left:?}");
    assert!(dest.join("5jls.html").is_file(), "the page was not written");
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&dest);
}

/// A store source, so `to_book` is exercised rather than only its parts.
mod store {
    use super::*;
    use crate::snapshot::{DocumentSource, Entry, Snapshot};
    use crate::store::{LocalPaths, StoreGraph, StoreRef};
    use std::path::{Path, PathBuf};

    struct Toy;

    #[derive(Debug, Clone)]
    struct ToyDoc {
        body: String,
    }

    impl Chaptered for ToyDoc {
        fn title(&self) -> String {
            self.body.lines().next().unwrap_or_default().to_string()
        }
        fn body(&self) -> &str {
            &self.body
        }
        fn group(&self) -> Option<String> {
            // A line reading `group: x` sets the sidebar group.
            self.body
                .lines()
                .find_map(|l| l.strip_prefix("group: "))
                .map(str::to_string)
        }
    }

    impl DocumentSource for Toy {
        type Doc = ToyDoc;

        fn load(
            &self,
            content: &crate::store::StoreContent,
            _skipped: &mut Vec<String>,
        ) -> crate::error::Result<Vec<Entry<ToyDoc>>> {
            let scan = content.scan("")?;
            let mut out = Vec::new();
            for entry in scan.entries {
                let text = content.read(&entry.path.to_string_lossy())?;
                out.push(Entry {
                    id: entry.stem,
                    doc: ToyDoc { body: text },
                });
            }
            Ok(out)
        }

        fn references(&self, _doc: &ToyDoc, _member: usize, _graph: &StoreGraph) -> Vec<StoreRef> {
            Vec::new()
        }

        fn resolve_local(&self, id: &str, entries: &[Entry<ToyDoc>]) -> Option<usize> {
            entries.iter().position(|e| e.id == id)
        }
    }

    fn setup(base: &Path, name: &str, yaml: &str, docs: &[(&str, &str)]) -> PathBuf {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stores.yml"), yaml).unwrap();
        for (id, body) in docs {
            std::fs::write(dir.join(format!("{id}.md")), body).unwrap();
        }
        dir
    }

    #[test]
    fn a_declared_store_s_documents_are_not_pages_of_this_book() {
        // A reader of this store follows a link to reach another one. A
        // foreign document rendered here would be a second copy that
        // drifts, and its page path could collide with a local id.
        let base = scratch("foreign");
        setup(
            &base,
            "mine",
            "stores:\n  - alias: theirs\n    path: ../theirs\n",
            &[
                ("local", "A local note\n"),
                ("shared", "Mine, not theirs\n"),
            ],
        );
        setup(&base, "theirs", "stores: []\n", &[("shared", "Theirs\n")]);

        let graph = StoreGraph::open(&base.join("mine"), &LocalPaths).unwrap();
        let snapshot = Snapshot::load(graph, &Toy).unwrap();
        let book = to_book(&snapshot);

        let paths: Vec<String> = book
            .items
            .iter()
            .filter_map(|item| match item {
                BookItem::Chapter(c) => Some(c.path.as_ref().unwrap().display().to_string()),
                _ => None,
            })
            .collect();
        paths
            .iter()
            .for_each(|p| assert!(!p.contains('/'), "{p} is foreign"));
        assert_eq!(paths.len(), 2, "expected the two local pages: {paths:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_group_becomes_a_part_title_and_the_pages_keep_their_paths() {
        let base = scratch("grouped");
        setup(
            &base,
            "mine",
            "stores: []\n",
            &[
                ("aaa", "First\ngroup: todo\n"),
                ("bbb", "Second\ngroup: done\n"),
                ("ccc", "Third, ungrouped\n"),
            ],
        );
        let graph = StoreGraph::open(&base.join("mine"), &LocalPaths).unwrap();
        let snapshot = Snapshot::load(graph, &Toy).unwrap();
        let book = to_book(&snapshot);

        let titles: Vec<String> = book
            .items
            .iter()
            .filter_map(|item| match item {
                BookItem::PartTitle(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(titles, vec!["done".to_string(), "todo".to_string()]);
        // Grouping is a sidebar, not a path. Every page is still flat.
        for item in &book.items {
            if let BookItem::Chapter(c) = item {
                let p = c.path.as_ref().unwrap().display().to_string();
                assert!(
                    ["aaa.md", "bbb.md", "ccc.md"].contains(&p.as_str()),
                    "{p} moved because of grouping"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[test]
fn an_id_that_climbs_out_writes_nothing_outside_the_destination() {
    // The refusal in chapter_of is the only thing between a hostile id
    // and a file beside the destination. Both call sites survived their
    // guard's deletion with the suite green, so this asserts the effect
    // rather than the predicate.
    let base = scratch("escape-base");
    let dest = base.join("site");
    std::fs::create_dir_all(&base).unwrap();

    let good = chapter_of("5jls", &doc("A page.")).expect("a plain id");
    let book = Book::new_with_items(vec![BookItem::Chapter(good)]);
    // A climbing id must not become a chapter at all.
    assert!(chapter_of("../escaped", &doc("Hostile.")).is_none());
    let d = Doc {
        title: "An issue",
        body: "",
        rows: Vec::new(),
        group: None,
        sub: vec![("Scratch", "text")],
    };
    assert!(
        chapter_of("../escaped", &d).is_none(),
        "a climbing id produced a chapter with sub-pages"
    );

    render_html(book, "A book", &dest).unwrap();
    let beside: Vec<String> = std::fs::read_dir(&base)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "site")
        .collect();
    assert!(beside.is_empty(), "the render wrote outside: {beside:?}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_planted_marker_does_not_license_deleting_a_directory() {
    // A marker was once enough on its own. One file licensed emptying
    // everything beside it, including a .git directory, because mdbook
    // exempts no dotfile from the delete.
    let dir = scratch("planted");
    std::fs::create_dir_all(dir.join(".git/objects")).unwrap();
    std::fs::write(dir.join(".git/objects/o"), "an object").unwrap();
    std::fs::write(dir.join("CNAME"), "notes.example.com").unwrap();
    std::fs::write(dir.join("thesis.md"), "a person's work").unwrap();
    std::fs::write(dir.join(MARKER), "written by mdstore\n").unwrap();

    let book = Book::new_with_items(vec![BookItem::Chapter(
        chapter_of("5jls", &doc("A page.")).unwrap(),
    )]);
    let err = render_html(book, "A book", &dir).expect_err("a forged marker is refused");
    assert!(err.to_string().contains("did not write"), "{err}");
    assert!(dir.join(".git/objects/o").is_file(), "the git object went");
    assert!(dir.join("CNAME").is_file(), "the CNAME went");
    assert!(dir.join("thesis.md").is_file(), "the thesis went");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_added_after_a_render_stops_the_next_one() {
    // The ordinary way to publish this output is to render, then set up
    // a repository in the destination. That must not arm a deletion.
    let dir = scratch("added");
    let book = || {
        Book::new_with_items(vec![BookItem::Chapter(
            chapter_of("5jls", &doc("A page.")).unwrap(),
        )])
    };
    render_html(book(), "A book", &dir).unwrap();
    std::fs::write(dir.join("CNAME"), "notes.example.com").unwrap();

    let err = render_html(book(), "A book", &dir).expect_err("an added file is refused");
    assert!(
        err.to_string().contains("CNAME"),
        "the refusal names it: {err}"
    );
    assert!(dir.join("CNAME").is_file(), "the CNAME went");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_id_holding_a_delimiter_or_a_url_character_has_no_page() {
    // Each of these was admitted by a denylist. A `>` closes the angle
    // brackets a destination is written with, and `#` and `?` are read
    // by a browser as a fragment and a query while the file keeps them.
    for bad in ["a>b", "a<b", "a#b", "a?b", "a\tb", "a b", "a%b", "a&b"] {
        assert!(page_path(bad).is_none(), "{bad:?} produced a page");
    }
    // The set an id may hold.
    for good in ["5jls", "a-b", "a_b", "a.b", "A1"] {
        assert!(page_path(good).is_some(), "{good:?} was refused");
    }
}

#[test]
fn a_stray_backtick_does_not_swallow_a_later_reference() {
    // An unmatched backtick is literal text in CommonMark. Treating it
    // as an opener lost every reference after it on the line.
    let out = rewrite_links("the ` char and [[5jls]]\n");
    assert!(
        out.contains("[5jls](<5jls.md>)"),
        "the link was lost: {out}"
    );
}

#[test]
#[cfg(unix)]
fn an_inventory_of_a_looping_symlink_terminates() {
    // `link -> .` is unbounded recursion for a walk that descends a
    // link. The walk reads link metadata and does not descend, so the
    // loop cannot form.
    let dir = scratch("loop");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("page.html"), "a page").unwrap();
    std::os::unix::fs::symlink(".", dir.join("link")).unwrap();

    let held = inventory(&dir).expect("the walk returns");
    assert!(held.contains(&"link".to_string()), "{held:?}");
    assert!(
        !held.iter().any(|p| p.starts_with("link/")),
        "the walk descended the link: {held:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refusal_names_what_to_do_about_it() {
    // A refusal a person cannot act on is a refusal they route around.
    let dir = scratch("remedy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("page.html"), "output with no marker").unwrap();
    let book = || {
        Book::new_with_items(vec![BookItem::Chapter(
            chapter_of("5jls", &doc("A page.")).unwrap(),
        )])
    };
    let err = render_html(book(), "A book", &dir).unwrap_err().to_string();
    assert!(
        err.contains("render again"),
        "no remedy for a stranded render: {err}"
    );
    assert!(
        err.contains("somewhere else"),
        "no remedy for a precious directory: {err}"
    );

    // The other refusal names the offending path and a remedy.
    let _ = std::fs::remove_dir_all(&dir);
    render_html(book(), "A book", &dir).unwrap();
    std::fs::write(dir.join(".DS_Store"), "finder").unwrap();
    let err = render_html(book(), "A book", &dir).unwrap_err().to_string();
    assert!(err.contains(".DS_Store"), "the path is not named: {err}");
    assert!(err.contains("Remove the path"), "no remedy: {err}");
    let _ = std::fs::remove_dir_all(&dir);
}
