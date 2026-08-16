# mdstore

A library for stores of YAML-frontmatter markdown documents. It is the
storage layer under [tisket](https://github.com/cjohnhanson/tisket),
[zettel](https://github.com/cjohnhanson/zettel), and
[almanac](https://github.com/cjohnhanson/almanac). Use it when you want
structured data in markdown files in git, and you do not want to write
the frontmatter parser, the link graph, or the composition rules again.

The crate has two halves. The document half parses and serializes one
file. The store half composes many stores into one graph you can read
from a single vantage point.

## Install

```sh
cargo add --git https://github.com/cjohnhanson/mdstore mdstore
```

Or in `Cargo.toml`:

```toml
[dependencies]
mdstore = { git = "https://github.com/cjohnhanson/mdstore" }
serde = { version = "1", features = ["derive"] }
```

## Documents

`Document<T>` holds typed frontmatter and a string body. The
frontmatter type is any `Serialize + DeserializeOwned`. `parse` reads
the YAML between the `---` fences. `serialize` writes the canonical
form back.

```rust
use mdstore::{parse, serialize, Document};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Note {
    title: String,
    tags: Vec<String>,
}

fn main() -> mdstore::Result<()> {
    let raw = "---\ntitle: Hello\ntags: [a, b]\n---\n\nbody text";
    let doc: Document<Note> = parse(raw)?;
    println!("{} {:?}", doc.frontmatter.title, doc.body);

    let out: String = serialize(&doc)?;
    let _: Document<Note> = parse(&out)?;
    Ok(())
}
```

## Provenance

The `provenance` module records who wrote each part of a body. A span
carries an origin: `human`, `agent`, or `citation`. A missing origin
means `unknown`, and nothing promotes it to `human`.

The markers are HTML comments, so any markdown reader ignores them:

```markdown
<!-- prov agent:summary -->
The trial enrolled 214 patients across nine sites.
<!-- /prov -->
```

`parse_spans` reads a body into spans. `render_spans` writes them back.
`ends_open` reports a body with a marker that no `/prov` closes.

## Stores and composition

A store is a directory with a `stores.yml` file. That file declares the
other stores this one may link into, under local aliases. A store sees
only the stores it declares, and the stores those declare in turn. This
makes a directed graph, and a cycle is allowed.

```yaml
format: 2
shared: false
stores:
  - alias: method
    git: https://github.com/example/method-notes
    rev: main
  - alias: archive
    path: ../archive
```

A declaration carries one source: `path`, `git` with an optional `rev`,
or `blob`. Set `shared: true` when other people clone the store.
`StoresConfig::unshareable` then reports a dependency path that only
resolves on the declaring machine.

`StoreGraph::open` walks that declaration into a closure of members.
A store's identity is its resolved source, not a name it gives itself,
so one store reached two ways stays one member and a cycle terminates.
An alias resolves through the alias table of the store that holds the
referring document, never through the vantage store's table.

`Snapshot::load` reads every document in the closure once and builds
the link graph over them. It answers `forward`, `backlinks`,
`neighborhood`, `orphans`, and `missing`.

## Sources

A member store can be local or remote:

- `path` — a directory on this machine.
- `git` — a bare clone in a per-URL cache slot, read at each consumer's
  declared rev through git objects. Two consumers that pin different
  revisions share one fetch, and neither overwrites the other. Only an
  explicit sync reaches the network. All of it runs in-process on gix:
  https and git:// over gix's own transports, and a local repository by
  reading its object database. No git process runs. An ssh URL is
  refused, because gix would spawn ssh for it; declare https.
- `blob` — an https prefix that publishes an `index.txt`, synced into a
  cache directory by plain GET. No vendor CLI runs; `s3://` and `gs://`
  are refused.

The `registry` module holds local overrides. An override changes where
a dependency resolves. It never changes what a store declares.

A remote store is third-party content. `StoreGraph` marks a member
remote transitively, and it refuses a local path that a remote member
declares.

## Selectors and slugs

`Selector::parse` reads a `key:value` filter, and `matches_all` applies
a set of them to one item.

For stable file names beside human-readable ones:

- `slugify("Fix the Widget!")` returns `"fix-the-widget"`
- `generate_prefix(&existing)` returns a 4-character id such as `"ab12"`, avoiding the ids already in `existing`
- `extract_prefix("ab12-fix-the-widget")` returns `Some(("ab12", "fix-the-widget"))`
- `has_prefix("ab12-fix-the-widget")` returns `true`

## Serving over MCP

The `mcp` feature carries the pieces the three tools share when they
serve a store to a Model Context Protocol client: the surface
configuration, the access mode, the document URI form, and the content
digest.

```toml
mdstore = { git = "https://github.com/cjohnhanson/mdstore", features = ["mcp"] }
```

The feature is off by default, so a consumer that builds only a CLI
does not pull in the server stack.

## Related

- [tisket](https://github.com/cjohnhanson/tisket) — file-based issue tracker built on mdstore
- [zettel](https://github.com/cjohnhanson/zettel) — zettelkasten built on mdstore
- [almanac](https://github.com/cjohnhanson/almanac) — agent skill aggregator built on mdstore
- [belmont](https://github.com/cjohnhanson/belmont) — secrets manager for LLM agents
- [codelikecody](https://github.com/cjohnhanson/codelikecody) — workflow engine that bundles these

## License

MIT.
