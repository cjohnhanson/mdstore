---
title: 'user-level config: a default root store and one precedence order across tisket, zettel, almanac'
status: done
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-16T22:47:30Z
updated: 2026-08-17T06:29:32Z
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
2026-08-16 22:5x: gaff ci implemented in ~/Projects/.wt/gaff-ci (2c2a6e6, unpushed): three phases (config, drift, hooks fail-closed), synthesized pre-push stdin+args from git files via gix-discover, uses/with workflow steps, action.yml, docs, missouri state ci-declared→ci-ran; 227 unit + 24 missouri green, clippy -D warnings clean. missouri action.yml written in .wt/missouri-action (uncommitted). Commit gate landed on origin: gaff 87ee4a7, missouri d8dc423 (with my own metadata+fmt commits — misattributed to the peer twice, corrected), almanac c860d19, mdstore e1e53cc; tisket push in flight; zettel waits behind the peer's push. Next: gaff-ci fresh-eyes review + land, missouri action land, gate workflows declared per repo, branch protection, co.d/hms.
2026-08-17 01:0x: commit gate on origin in all six repos (fmt --check + clippy -D warnings + index/tree divergence refusal), two fresh-eyes rounds, each push through its merge gate. Root cause of the tisket push failures found: GitHub SSH idle-closes during a long pre-push gate under load; ServerAliveInterval set for github.com (machine-local ~/.ssh/config), zettel note filed. Gate output-truncation issue filed in gaff. commit-gate hold cleared. Still open: gaff ci review+land (reviewer cut off twice by machine sleep; restart), missouri action, gate workflows, branch protection, co.d/hms.
2026-08-17: DONE. hms deployed the four tools; installed binaries verified live: fallback read announces, write refuses naming --home, --home acts on the root, store root shows the seam, real config untouched. Follow-ups filed earlier stay open (recursive scan, AGENTS.md, skills-spec audit, overlay linking, remote awareness).
