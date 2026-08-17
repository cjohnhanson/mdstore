---
title: 'a relative git: declaration resolves against cwd and keys its slot by literal text'
status: done
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-16T21:02:18Z
updated: 2026-08-17T18:16:08Z
---

## Problem

`git: ../up` resolves against the process cwd (as the git CLI clone did), and the slot name hashes the literal text, so two roots declaring `../up` share one slot and store identity dedups them. Found by QA on 2026-08-16. Pre-existing; the gix rewrite kept the behavior.

## Fix

Resolve a local `git:` URL against the declaring `stores.yml` before it reaches `git.rs`, and key the slot and the identity by the resolved absolute path. `sync_source` needs the declaring root for that.

## Scratch Notes

## 2026-08-17 — fixed, in review

Branch fix/mirror-packs-the-create, awaiting an independent verdict.

The fix follows the issue: the graph walk resolves a local git url
against the declaring root, and locate, identity and a consumer's sync
all see one absolute path. It runs after the location guards, so a
dependency is still judged on the text it declared rather than on what
that text resolves to.

One correction the first review forced: the rewrite path-joined
file:// text too, because is_remote_url is deliberately false for it.
A root-level git: file:///abs/repo — the legitimate case — became a
member no sync could satisfy. Text carrying a scheme is now left
alone. That regression existed only on the branch and never reached
main.
Closed at mdstore a0a8c51, landed through the check-gated path with a
review note naming every mutation two independent reviewers killed.
