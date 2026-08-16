---
title: 'user-level config: a default root store and one precedence order across tisket, zettel, almanac'
status: todo
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-16T22:47:30Z
updated: 2026-08-16T22:47:30Z
---

## Goal

Each tool finds its store the same way, in one documented order, and a user can name a default root store so `zettel note create` from a repo with no store lands in the user's private store rather than failing. Config has defaults for every key. The design is agent-first: an agent in any cwd can tell where a write goes and where a read comes from, and the answer is the same across the three tools.

## Facts today

- tisket, zettel, almanac have no user-level config; each roots at `--root` or the cwd and reads `<tool>.yml` there.
- gaff: `~/.config/gaff/` by fixed name (security reasoning in handler.rs). mdstore: `~/.config/mdstore/registry.yml` (XDG honored) and `~/.cache/mdstore/`.
- mdstore's model: a store declares dependencies; a dependency never sees up. The registry redirects a declared URL to a local checkout.

## Larger shape this serves

A private plaintext repo as the root store for all three tools, declaring per-project stores (committed ones, and private overlays kept out of a project via .git/info/exclude but tracked in the private repo), a public store later; remote awareness (ahead/behind, other branches) via gix as a follow-up.

## Not in scope here

Overlay linking mechanics, AGENTS.md in gaff, skills-spec audit, recursive scan (each is its own issue).

## Scratch Notes

2026-08-16 18:00: adversarial design workflow running (two designers minimal-vs-explicit, three attack lenses on agent ergonomics/spoof/graph-consistency). In parallel: merge gate landed in six repos (.gaff/gaff.yml pre-push -> scripts/merge-gate.sh: cargo test + missouri suite + fresh-eyes git note on the tip), proven to refuse an unreviewed push in gaff; fresh-eyes reviewer on the gate itself is running; pushes of the gate commits wait on that review.
