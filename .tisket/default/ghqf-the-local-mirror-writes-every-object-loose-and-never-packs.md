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
