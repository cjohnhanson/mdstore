---
title: the local mirror writes every object loose and never packs
status: todo
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-16T21:02:18Z
updated: 2026-08-16T21:02:18Z
---

## Problem

`mirror_local` writes each copied object loose; a 68 MB source produced 10,395 loose objects and a 12 s first sync. Correct, but the slot grows as loose files. Also an interrupted create leaves `<slot>.tmp-<pid>` that no later run removes. Found by QA on 2026-08-16.

## Fix

Write a pack for the copied set (gix-pack), or copy the source's pack files when the source is bare/packed and only loose the delta. Sweep stale `tmp-*` siblings whose pid is gone.

## Scratch Notes

## 2026-08-17 — both halves fixed, in review

Branch fix/mirror-packs-the-create, on mdstore main, awaiting an
independent verdict.

The pack half took the first option the issue named, not the second.
The mirror already computes the exact closure, so on a create the ids
collect and one pack is written from them through gix-pack's entries
pipeline and bundle writer. Copying the source's pack files was the
alternative; it only helps when the source is already packed, and it
would still leave a loose delta path to maintain. An update keeps the
loose write, because packing every small delta trades one file
explosion for another.

The sweep half deviates from the issue deliberately. The issue asked
for a pid-liveness check; the fix sweeps a staging sibling whose mtime
is older than an hour. A pid check needs a new dependency, a live
peer's staging is minutes old, and a wrong delete costs one failed
rename rather than corruption. Recorded because the deviation should
be visible to whoever reads this next.

Both halves are pinned by tests written before the fix, and each
guard dies under its own mutation.
