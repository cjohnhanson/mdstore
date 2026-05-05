# mdstore

Parser and serializer for YAML-frontmatter markdown documents. The
storage layer underneath [tisket](https://github.com/cjohnhanson/tisket)
and [zettel](https://github.com/cjohnhanson/zettel). Use it when you
want to store structured data as markdown files in git instead of
hand-rolling frontmatter parsing.

`Document<T>` holds typed frontmatter (any `Serialize + DeserializeOwned`)
and a string body. `parse` extracts the YAML between `---` fences;
`serialize` reconstructs canonical format. Also includes slug
generation and prefix-handling utilities.

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

## Usage

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

## Slug + prefix utilities

For projects that want stable file IDs alongside human-readable names:

- `slugify("Fix the Widget!")` returns `"fix-the-widget"`
- `generate_prefix()` returns a 4-char random ID like `"ab12"`
- `extract_prefix("ab12-fix-the-widget")` returns `Some(("ab12", "fix-the-widget"))`
- `has_prefix("ab12-fix-the-widget")` returns `true`

## Related

- [tisket](https://github.com/cjohnhanson/tisket) — file-based issue tracker built on mdstore
- [zettel](https://github.com/cjohnhanson/zettel) — zettelkasten built on mdstore
- [almanac](https://github.com/cjohnhanson/almanac) — agent skill aggregator
- [belmont](https://github.com/cjohnhanson/belmont) — secrets manager for LLM agents
- [codelikecody](https://github.com/cjohnhanson/codelikecody) — workflow engine that bundles these

## License

MIT.
