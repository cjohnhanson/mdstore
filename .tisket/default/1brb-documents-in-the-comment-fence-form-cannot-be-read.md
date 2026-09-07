---
title: documents in the comment fence form cannot be read
status: todo
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-19T02:10:42Z
updated: 2026-08-19T02:10:42Z
---

A documentation page is read on a git forge as well as through its own tool. A forge renders a leading `---` as a horizontal rule followed by the raw keys, so every page grows a visible block of metadata. tisket, zettel, almanac, and missouri already avoid that: each `docs/*.md` page carries its metadata in an HTML comment.

```
<!-- metadata
title: "What is Tisket?"
description: "Why plaintext issue tracking and how tisket's design works"
type: explanation
-->
```

mdstore parses `---` and nothing else, so no tool can read those pages as documents. A fifth tool, diataxis, manages exactly those pages and needs to.

Add the comment form as a second fence, alongside `---`. `---` stays the default, so no existing caller changes.

## Surface

- `Fence`, with `Yaml` and `Comment`, plus `open`, `close`, and `detect`.
- `split_with`, `parse_with`, `serialize_with`, each taking a fence.
- `parse_any`, which detects the form and reports it, so an edit writes a document back in the form it came in.
- `split`, `parse`, and `serialize` keep their signatures and their behaviour.

Additive only. No dependent tool needs a change.

## Scratch Notes

Implemented on branch feat/comment-fence, staged and not committed.

src/document.rs gains Fence, strip_open, split_with, parse_with, parse_any, serialize_with. split_fences now takes a fence. split, parse, and serialize delegate with Fence::Yaml, so their behaviour is unchanged. src/lib.rs re-exports the new names.

Ten tests added: a comment-fenced parse, detect on both forms and on neither, a delimiter with trailing text opening nothing, parse_any reporting its fence, a comment round trip, a value holding --> surviving a round trip, an unclosed comment fence erroring, each fence refusing the other form, and split_with giving verbatim frontmatter.

Gates: cargo fmt --check clean, clippy --workspace --all-targets --all-features -D warnings clean, 220 tests pass and 1 is ignored.

Not done: a push needs a review note written by someone other than the author of the change.
