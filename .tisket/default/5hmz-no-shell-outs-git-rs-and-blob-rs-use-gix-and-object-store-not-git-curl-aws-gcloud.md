---
title: 'no shell-outs: git.rs and blob.rs use gix and object_store, not git/curl/aws/gcloud'
status: in_progress
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-16T19:54:34Z
updated: 2026-08-16T19:54:56Z
---

## Goal

mdstore is a single binary. `src/git.rs` (bare-clone cache: clone, fetch, rev-parse, ls-tree, show, origin url) and `src/blob.rs` (curl, aws s3, gcloud storage) spawn external programs. Both move to libraries: gix for git, one object-store library for s3/gs/https. No `std::process::Command` remains in mdstore.

## Why

The ecosystem rule from day one: single binary, no shell-outs, unless the thing spawned is closed source (claude) or a user-declared command. clc has held this as a test (`no_subprocess_git_calls`) since its start; mdstore, gaff, and almanac broke it on 2026-08-12 and 2026-08-14. Caught by the user 2026-08-16.

## Scope

- git.rs: every function keeps its signature and semantics; the body uses gix. Bare clone into the cache slot, fetch with prune of `refs/heads/*`, rev resolution with `^{commit}` semantics, tree listing under a prefix, blob read, origin URL from config.
- blob.rs: https index + documents, s3, gs through one library. Same staging-then-rename cache discipline.
- Tests: the unit tests that build a real repository do so with gix, not `git init`; missouri suites in tisket/zettel/almanac that use `git init` for fixtures may keep it (fixtures, not the binary).
- Known library facts to state in the docs, not hide: gix's ssh transport delegates to the ssh program; gix's https auth uses git credential helpers. If either is unacceptable, that is a separate decision.

## Not in scope

Writes to remote stores; a store that is a remote; sync of writes.

## Related

gaff: `githook::hooks_dir` and `handler` kill. almanac: `vendor.rs`, `ops.rs` diff. clc: `supervisor.rs` `sh -c claude`.
