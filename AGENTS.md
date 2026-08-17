# mdstore

A Rust library. It stores YAML-frontmatter markdown documents, composes
stores into a graph, and resolves documents across that graph. It ships
no binary.

Three tools depend on it: tisket, zettel, and almanac. Each pins a
revision. A breaking change here breaks their next pin bump.

Read [README.md](README.md) for the document model, provenance, store
composition, and the user layer. Read
[CONTRIBUTING.md](CONTRIBUTING.md) for the gates and the pull-request
rules. This file covers only what those two do not.

## Before you push

Run the checks the gates run. Pass `--all-features` to both. The `mcp`
feature is off by default, so a run without it never compiles that code:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

A push also needs a review note on its tip. Do not write the note for
your own change. The gate greps for the words. It cannot check who
wrote them. See CONTRIBUTING.md.

## Rules

1. **Match the existing pattern.** This codebase has settled shapes for
   errors, path handling, and store resolution. Read a neighbouring
   module before adding a third way to do something it already does.
2. **Use the codebase's terms.** A store, a document, a selector, a
   provenance span, a slug. Do not introduce a synonym for any of them.
3. **Never commit an absolute path.** It exposes an account layout, and
   it breaks every other clone. Build a path from a temporary directory
   in a test, or use a relative one.
4. **Never commit a local dependency override.** A `path = "..."` that
   points outside the repository belongs in `.cargo/config.toml`.
5. **Do not weaken a gate to make a change pass.** If a gate is wrong,
   change the gate in its own commit and say why.
6. **Do not silence a lint without a reason on the same line.**
7. **Ask before a public API change.** Three tools depend on this
   library, and a signature change breaks their builds.

## Tests

Unit tests sit beside the code they cover. Integration tests live in
`tests/`.

A bug fix carries a test that fails before the fix. Name that test in
the commit message.

A test for a guard is verified by removing the guard. If the test still
passes, it does not test the guard.

## Where the work is tracked

Issues are markdown files under `.tisket/`. Read them with `cat`, or
with tisket if it is installed:

```sh
tisket issue list
tisket issue show <id>
tisket scratch <id> read
```

Append to the scratch of the issue you are working as the state changes.
The next session starts from what you leave there.

## Commit messages

Imperative present. One topic. State what the change does, and why where
the diff does not show it.

Do not narrate the process. Do not reference the conversation that
produced the change.

Add a trailer when a coding assistant wrote a substantial part:

```
Co-authored-by: Claude <noreply@anthropic.com>
```
