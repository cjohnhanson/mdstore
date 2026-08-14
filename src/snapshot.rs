//! A loaded view of a store graph.
//!
//! The snapshot reads each store one time. A flat table holds the
//! documents, with the key `(member, id)`. The consumer reports the
//! forward edges, and the snapshot calculates the reverse index one
//! time. Backlinks, orphans, and neighborhoods are then lookups. The
//! snapshot does not read the stores again.
//!
//! The consumer supplies the vocabulary through [`DocumentSource`].
//! This module does not know what an ID means, how a body makes a
//! citation, or what the frontmatter holds.

use std::collections::HashMap;

use crate::error::Result;
use crate::store::{Member, StoreGraph, StoreRef};

/// Where a document sits: which closure member, and its ID there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocId {
    pub member: usize,
    pub index: usize,
}

/// One loaded document.
#[derive(Debug, Clone)]
pub struct Entry<D> {
    pub id: String,
    pub doc: D,
}

/// Reads the documents of one store and reports their outbound
/// references. Implemented by the consumer, which owns ID resolution.
pub trait DocumentSource {
    /// The consumer's document type.
    type Doc;

    /// Load all the documents of a store from its root directory.
    /// If a document is not readable, do not raise an error. Skip the
    /// document, and report it through `skipped`. One bad file must not
    /// stop a whole store.
    fn load(
        &self,
        root: &std::path::Path,
        skipped: &mut Vec<String>,
    ) -> Result<Vec<Entry<Self::Doc>>>;

    /// The references a document makes, as written in it.
    ///
    /// The alias set of the declaring store parses the reference. This
    /// is the reason for the member index. An alias has the meaning that
    /// the store with the document gives it.
    fn references(&self, doc: &Self::Doc, member: usize, graph: &StoreGraph) -> Vec<StoreRef>;

    /// References that make an edge if they resolve, but that do not
    /// have to resolve. A citation source can name a note, or it can be
    /// an external key with no meaning in this graph. These references
    /// never appear in [`Snapshot::dangling`].
    fn optional_references(
        &self,
        _doc: &Self::Doc,
        _member: usize,
        _graph: &StoreGraph,
    ) -> Vec<StoreRef> {
        Vec::new()
    }

    /// Resolve an ID in one store. The consumer owns this vocabulary:
    /// prefixes, slugs, and exact IDs.
    fn resolve_local(&self, id: &str, entries: &[Entry<Self::Doc>]) -> Option<usize>;
}

/// A loaded closure: documents, edges, and what failed to load.
pub struct Snapshot<S: DocumentSource> {
    pub graph: StoreGraph,
    entries: Vec<Vec<Entry<S::Doc>>>,
    forward: HashMap<DocId, Vec<DocId>>,
    reverse: HashMap<DocId, Vec<DocId>>,
    /// References that named nothing, by the document that made them.
    pub dangling: Vec<(DocId, StoreRef)>,
    /// Documents and stores that could not be read.
    pub skipped: Vec<String>,
}

impl<S: DocumentSource> Snapshot<S> {
    /// Load every reachable member once and derive the edge indexes.
    pub fn load(graph: StoreGraph, source: &S) -> Result<Self> {
        let mut entries: Vec<Vec<Entry<S::Doc>>> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();

        for member in &graph.members {
            match &member.root {
                Some(root) => {
                    let mut member_skipped = Vec::new();
                    let loaded = source.load(root, &mut member_skipped)?;
                    for s in member_skipped {
                        skipped.push(format!("{}: {s}", label(member)));
                    }
                    entries.push(loaded);
                }
                None => {
                    skipped.push(format!(
                        "{}: {}",
                        label(member),
                        member.unavailable.as_deref().unwrap_or("unavailable")
                    ));
                    entries.push(Vec::new());
                }
            }
        }

        let mut snapshot = Snapshot {
            graph,
            entries,
            forward: HashMap::new(),
            reverse: HashMap::new(),
            dangling: Vec::new(),
            skipped,
        };
        snapshot.index_edges(source);
        Ok(snapshot)
    }

    fn index_edges(&mut self, source: &S) {
        let mut forward: HashMap<DocId, Vec<DocId>> = HashMap::new();
        let mut reverse: HashMap<DocId, Vec<DocId>> = HashMap::new();
        let mut dangling = Vec::new();

        for member in 0..self.entries.len() {
            for index in 0..self.entries[member].len() {
                let from = DocId { member, index };
                let doc = &self.entries[member][index].doc;
                let required = source.references(doc, member, &self.graph);
                let optional = source.optional_references(doc, member, &self.graph);
                for (r, required) in required
                    .into_iter()
                    .map(|r| (r, true))
                    .chain(optional.into_iter().map(|r| (r, false)))
                {
                    match self.resolve_from(member, &r, source) {
                        Some(to) if to != from => {
                            forward.entry(from).or_default().push(to);
                            reverse.entry(to).or_default().push(from);
                        }
                        Some(_) => {}
                        None if required => dangling.push((from, r)),
                        None => {}
                    }
                }
            }
        }

        for list in forward.values_mut().chain(reverse.values_mut()) {
            list.sort();
            list.dedup();
        }
        self.forward = forward;
        self.reverse = reverse;
        self.dangling = dangling;
    }

    /// Resolve a reference as written in a document of `from_member`.
    ///
    /// A bare reference resolves in that member. An alias resolves
    /// through the declarations of that member. It does not use the
    /// declarations of the root store, so a dependency cannot use the
    /// alias table of the root.
    pub fn resolve_from(&self, from_member: usize, r: &StoreRef, source: &S) -> Option<DocId> {
        let mut member = from_member;
        for alias in &r.alias {
            member = self.graph.target_of(member, alias)?;
        }
        let index = source.resolve_local(&r.id, &self.entries[member])?;
        Some(DocId { member, index })
    }

    /// Every document, in member then load order.
    pub fn documents(&self) -> impl Iterator<Item = (DocId, &Entry<S::Doc>)> {
        self.entries.iter().enumerate().flat_map(|(member, list)| {
            list.iter()
                .enumerate()
                .map(move |(index, e)| (DocId { member, index }, e))
        })
    }

    /// The documents of one member.
    pub fn member_documents(&self, member: usize) -> impl Iterator<Item = (DocId, &Entry<S::Doc>)> {
        self.entries[member]
            .iter()
            .enumerate()
            .map(move |(index, e)| (DocId { member, index }, e))
    }

    /// One document.
    pub fn get(&self, id: DocId) -> Option<&Entry<S::Doc>> {
        self.entries.get(id.member)?.get(id.index)
    }

    /// The documents this one references.
    pub fn forward(&self, id: DocId) -> &[DocId] {
        self.forward.get(&id).map_or(&[], |v| v.as_slice())
    }

    /// The documents that reference this one. Derived once at load, not
    /// recomputed per query.
    pub fn backlinks(&self, id: DocId) -> &[DocId] {
        self.reverse.get(&id).map_or(&[], |v| v.as_slice())
    }

    /// Documents with no edges in either direction.
    pub fn orphans(&self) -> Vec<DocId> {
        self.documents()
            .map(|(id, _)| id)
            .filter(|id| self.forward(*id).is_empty() && self.backlinks(*id).is_empty())
            .collect()
    }

    /// The neighborhood of a document within `depth` hops, following
    /// edges in both directions. The starting document comes first.
    pub fn neighborhood(&self, start: DocId, depth: usize) -> Vec<DocId> {
        let mut seen = vec![start];
        let mut frontier = vec![start];
        for _ in 0..depth {
            let mut next = Vec::new();
            for id in &frontier {
                for neighbor in self.forward(*id).iter().chain(self.backlinks(*id)) {
                    if !seen.contains(neighbor) {
                        seen.push(*neighbor);
                        next.push(*neighbor);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        seen
    }

    /// How a document prints from the root's vantage: bare when local,
    /// alias-qualified otherwise. Every printed ID must resolve back
    /// through the same grammar it was printed in.
    pub fn qualify(&self, id: DocId) -> String {
        let Some(entry) = self.get(id) else {
            return String::new();
        };
        self.graph.members[id.member].qualify(&entry.id)
    }

    /// True when the document belongs to a dependency, not the vantage
    /// store. Dependencies are read-only through the closure.
    pub fn is_foreign(&self, id: DocId) -> bool {
        id.member != 0
    }

    /// Members that could not be loaded, for the partial-result notice
    /// every graph-derived answer must carry.
    pub fn missing(&self) -> Vec<String> {
        self.graph.unavailable().iter().map(|m| label(m)).collect()
    }
}

fn label(member: &Member) -> String {
    if member.alias_path.is_empty() {
        "<root>".to_string()
    } else {
        member.alias_path.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{LocalPaths, STORES_FILE};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    /// A toy consumer: a document is a title plus a list of reference
    /// strings, one per line after the title.
    struct Toy {
        reads: RefCell<usize>,
    }

    #[derive(Debug, Clone)]
    struct ToyDoc {
        refs: Vec<String>,
    }

    impl DocumentSource for Toy {
        type Doc = ToyDoc;

        fn load(&self, root: &Path, skipped: &mut Vec<String>) -> Result<Vec<Entry<ToyDoc>>> {
            *self.reads.borrow_mut() += 1;
            let scan = crate::store::scan_documents(root)?;
            for (path, why) in scan.skipped {
                skipped.push(format!("{} ({why})", path.display()));
            }
            let mut out = Vec::new();
            for entry in scan.entries {
                let text = std::fs::read_to_string(&entry.path)?;
                out.push(Entry {
                    id: entry.stem,
                    doc: ToyDoc {
                        refs: text
                            .lines()
                            .filter(|l| !l.trim().is_empty())
                            .map(|l| l.trim().to_string())
                            .collect(),
                    },
                });
            }
            Ok(out)
        }

        fn references(&self, doc: &ToyDoc, member: usize, graph: &StoreGraph) -> Vec<StoreRef> {
            let aliases = graph.config(member).aliases();
            doc.refs
                .iter()
                .map(|r| StoreRef::parse(r, &aliases))
                .collect()
        }

        fn resolve_local(&self, id: &str, entries: &[Entry<ToyDoc>]) -> Option<usize> {
            entries.iter().position(|e| e.id == id)
        }
    }

    fn toy() -> Toy {
        Toy {
            reads: RefCell::new(0),
        }
    }

    /// (store name, its stores.yml, its documents as (id, body)).
    type StoreSpec<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

    fn setup(spec: &[StoreSpec<'_>]) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "mdstore-snap-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        for (name, yaml, docs) in spec {
            let dir = base.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(STORES_FILE), yaml).unwrap();
            for (doc, body) in *docs {
                std::fs::write(dir.join(format!("{doc}.md")), body).unwrap();
            }
        }
        base
    }

    fn load(base: &Path, root: &str, source: &Toy) -> Snapshot<Toy> {
        let graph = StoreGraph::open(&base.join(root), &LocalPaths).unwrap();
        Snapshot::load(graph, source).unwrap()
    }

    #[test]
    fn each_store_is_read_exactly_once() {
        let base = setup(&[
            (
                "user",
                "stores:\n  - alias: repo\n    path: ../repo\n",
                &[("note", "repo:target")],
            ),
            ("repo", "stores: []\n", &[("target", "")]),
        ]);
        let source = toy();
        let snap = load(&base, "user", &source);
        assert_eq!(*source.reads.borrow(), 2, "one read per member");
        // Repeated graph queries do not re-read.
        let start = snap.documents().next().unwrap().0;
        snap.backlinks(start);
        snap.orphans();
        snap.neighborhood(start, 3);
        assert_eq!(*source.reads.borrow(), 2, "queries hit the index, not disk");
    }

    #[test]
    fn cross_store_references_make_edges_both_ways() {
        let base = setup(&[
            (
                "user",
                "stores:\n  - alias: repo\n    path: ../repo\n",
                &[("note", "repo:target")],
            ),
            ("repo", "stores: []\n", &[("target", "")]),
        ]);
        let source = toy();
        let snap = load(&base, "user", &source);
        let note = DocId {
            member: 0,
            index: 0,
        };
        let target = DocId {
            member: 1,
            index: 0,
        };
        assert_eq!(snap.forward(note), &[target]);
        assert_eq!(
            snap.backlinks(target),
            &[note],
            "visible from the user vantage"
        );
        assert!(snap.dangling.is_empty());
    }

    #[test]
    fn the_dependency_cannot_see_its_dependents() {
        // The soundness rule in practice: from the repo's own vantage,
        // the user store is not in the closure, so the annotation is
        // invisible and the target is an orphan.
        let base = setup(&[
            (
                "user",
                "stores:\n  - alias: repo\n    path: ../repo\n",
                &[("note", "repo:target")],
            ),
            ("repo", "stores: []\n", &[("target", "")]),
        ]);
        let source = toy();
        let snap = load(&base, "repo", &source);
        assert_eq!(snap.graph.members.len(), 1);
        let target = DocId {
            member: 0,
            index: 0,
        };
        assert!(snap.backlinks(target).is_empty());
        assert_eq!(snap.orphans(), vec![target]);
    }

    #[test]
    fn bare_references_resolve_in_their_own_store() {
        // Both stores hold a document with the same ID. Each store's
        // bare reference must find its own.
        let base = setup(&[
            (
                "user",
                "stores:\n  - alias: repo\n    path: ../repo\n",
                &[("shared", "local"), ("local", "")],
            ),
            (
                "repo",
                "stores: []\n",
                &[("shared", "local"), ("local", "")],
            ),
        ]);
        let source = toy();
        let snap = load(&base, "user", &source);
        let user_shared = DocId {
            member: 0,
            index: 1,
        };
        let repo_shared = DocId {
            member: 1,
            index: 1,
        };
        assert_eq!(snap.get(user_shared).unwrap().id, "shared");
        assert_eq!(snap.get(repo_shared).unwrap().id, "shared");
        assert_eq!(
            snap.forward(user_shared),
            &[DocId {
                member: 0,
                index: 0
            }]
        );
        assert_eq!(
            snap.forward(repo_shared),
            &[DocId {
                member: 1,
                index: 0
            }]
        );
    }

    #[test]
    fn an_alias_resolves_through_the_declaring_store() {
        // The confused-deputy case: root and dep both declare 'kb'. A
        // reference written in the dep must reach the dep's kb.
        let base = setup(&[
            (
                "root",
                "stores:\n  - alias: dep\n    path: ../dep\n  - alias: kb\n    path: ../root-kb\n",
                &[("r", "kb:doc")],
            ),
            (
                "dep",
                "stores:\n  - alias: kb\n    path: ../dep-kb\n",
                &[("d", "kb:doc")],
            ),
            ("root-kb", "stores: []\n", &[("doc", "")]),
            ("dep-kb", "stores: []\n", &[("doc", "")]),
        ]);
        let source = toy();
        let snap = load(&base, "root", &source);

        let member_of = |name: &str| {
            snap.graph
                .members
                .iter()
                .position(|m| m.root.as_ref().is_some_and(|r| r.ends_with(name)))
                .unwrap()
        };
        let root_doc = DocId {
            member: 0,
            index: 0,
        };
        let dep_doc = DocId {
            member: member_of("dep"),
            index: 0,
        };
        assert_eq!(snap.forward(root_doc)[0].member, member_of("root-kb"));
        assert_eq!(
            snap.forward(dep_doc)[0].member,
            member_of("dep-kb"),
            "the dep's alias table wins for the dep's document"
        );
    }

    #[test]
    fn an_undeclared_alias_dangles_instead_of_resolving() {
        let base = setup(&[
            (
                "user",
                "stores:\n  - alias: repo\n    path: ../repo\n",
                &[("note", "nope:target")],
            ),
            ("repo", "stores: []\n", &[("target", "")]),
        ]);
        let source = toy();
        let snap = load(&base, "user", &source);
        // 'nope:target' is not a declared alias, so it stays an opaque
        // local ID, and no local document has that ID.
        assert_eq!(snap.dangling.len(), 1);
        assert_eq!(snap.dangling[0].1.to_string(), "nope:target");
        assert!(snap
            .forward(DocId {
                member: 0,
                index: 0
            })
            .is_empty());
    }

    #[test]
    fn a_missing_store_is_reported_and_the_rest_still_loads() {
        let base = setup(&[(
            "user",
            "stores:\n  - alias: gone\n    path: ../gone\n",
            &[("note", "")],
        )]);
        let source = toy();
        let snap = load(&base, "user", &source);
        assert_eq!(snap.missing(), vec!["gone".to_string()]);
        assert_eq!(snap.documents().count(), 1, "the local store still loaded");
    }

    #[test]
    fn qualified_ids_round_trip() {
        let base = setup(&[
            (
                "user",
                "stores:\n  - alias: repo\n    path: ../repo\n",
                &[("note", "")],
            ),
            (
                "repo",
                "stores:\n  - alias: deep\n    path: ../deep\n",
                &[("target", "")],
            ),
            ("deep", "stores: []\n", &[("far", "")]),
        ]);
        let source = toy();
        let snap = load(&base, "user", &source);
        assert_eq!(
            snap.qualify(DocId {
                member: 0,
                index: 0
            }),
            "note"
        );
        assert_eq!(
            snap.qualify(DocId {
                member: 1,
                index: 0
            }),
            "repo:target"
        );
        assert_eq!(
            snap.qualify(DocId {
                member: 2,
                index: 0
            }),
            "repo/deep:far",
            "a transitive document is addressable in output"
        );
        // The printed form parses back to the same document.
        let aliases = snap.graph.config(0).aliases();
        let parsed = StoreRef::parse("repo/deep:far", &aliases);
        assert_eq!(
            snap.resolve_from(0, &parsed, &source),
            Some(DocId {
                member: 2,
                index: 0
            })
        );
    }

    #[test]
    fn neighborhood_crosses_store_boundaries() {
        let base = setup(&[
            (
                "user",
                "stores:\n  - alias: repo\n    path: ../repo\n",
                &[("note", "repo:mid")],
            ),
            ("repo", "stores: []\n", &[("mid", "far"), ("far", "")]),
        ]);
        let source = toy();
        let snap = load(&base, "user", &source);
        let note = DocId {
            member: 0,
            index: 0,
        };
        assert_eq!(snap.neighborhood(note, 1).len(), 2);
        assert_eq!(snap.neighborhood(note, 2).len(), 3, "two hops, two stores");
    }

    #[test]
    fn foreign_documents_are_marked() {
        let base = setup(&[
            (
                "user",
                "stores:\n  - alias: repo\n    path: ../repo\n",
                &[("note", "")],
            ),
            ("repo", "stores: []\n", &[("target", "")]),
        ]);
        let source = toy();
        let snap = load(&base, "user", &source);
        assert!(!snap.is_foreign(DocId {
            member: 0,
            index: 0
        }));
        assert!(snap.is_foreign(DocId {
            member: 1,
            index: 0
        }));
    }

    #[test]
    fn a_diamond_counts_the_shared_store_once() {
        let base = setup(&[
            (
                "a",
                "stores:\n  - alias: b\n    path: ../b\n  - alias: c\n    path: ../c\n",
                &[("x", "")],
            ),
            ("b", "stores:\n  - alias: d\n    path: ../d\n", &[("y", "")]),
            ("c", "stores:\n  - alias: d\n    path: ../d\n", &[("z", "")]),
            ("d", "stores: []\n", &[("shared", "")]),
        ]);
        let source = toy();
        let snap = load(&base, "a", &source);
        let shared: Vec<_> = snap.documents().filter(|(_, e)| e.id == "shared").collect();
        assert_eq!(shared.len(), 1, "one document, not one per path to it");
        assert_eq!(*source.reads.borrow(), 4, "four stores, four reads");
    }
}
