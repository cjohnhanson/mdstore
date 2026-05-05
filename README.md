# mdstore

Generic parser and serializer for YAML-frontmatter markdown documents.
The storage layer underneath [tisket](https://github.com/cjohnhanson/tisket)
and [zettel](https://github.com/cjohnhanson/zettel).

`Document<T>` holds typed frontmatter (any `Serialize + Deserialize`)
and a string body. `parse` extracts the YAML between `---` fences and
returns the rest as body. `serialize` reconstructs canonical format.

Any tool that stores structured data as markdown files in git can use
this instead of hand-rolling frontmatter parsing. Also provides slug
generation, prefix handling, and document selection utilities.

## Install

```sh
cargo add --git https://github.com/cjohnhanson/mdstore mdstore
```

Or in `Cargo.toml`:

```toml
[dependencies]
mdstore = { git = "https://github.com/cjohnhanson/mdstore" }
```

## Usage

```rust
use mdstore::{Document, parse, serialize};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct Note {
    title: String,
    tags: Vec<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw = "---\ntitle: Hello\ntags: [a, b]\n---\nbody text\n";
    let doc: Document<Note> = parse(raw)?;
    println!("{:?} body={:?}", doc.frontmatter, doc.body);

    let out: String = serialize(&doc)?;
    assert_eq!(out, raw);
    Ok(())
}
```

## Slug + prefix utilities

For projects that want stable file IDs alongside human-readable names:

- `slugify("Fix the Widget!") -> "fix-the-widget"`
- `generate_prefix() -> "ab12"` (4-char random ID)
- `extract_prefix("ab12-fix-the-widget")` returns `Some(("ab12", "fix-the-widget"))`
- `has_prefix("ab12-...")` returns `true`

## Related

Part of a loose ecosystem of plaintext, git-tracked, agent-readable
tooling.

- [tisket](https://github.com/cjohnhanson/tisket) — issue tracker built on this
- [zettel](https://github.com/cjohnhanson/zettel) — zettelkasten built on this
- [almanac](https://github.com/cjohnhanson/almanac) — agent skill aggregator
- [belmont](https://github.com/cjohnhanson/belmont) — secrets manager
- [codelikecody](https://github.com/cjohnhanson/codelikecody) — workflow engine

## License

MIT.
