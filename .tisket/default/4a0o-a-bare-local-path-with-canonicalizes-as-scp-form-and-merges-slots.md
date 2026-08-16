---
title: a bare local path with @ canonicalizes as scp form and merges slots
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

`canonical_url` treats any `@` in a scheme-less declaration as scp form: `/x/at1/x@1` and `/x/at2/y@1` both canonicalize to `1` and land in one slot; identity dedup drops the second declaration silently. Also lowercases local paths, which merges two repositories that differ by case on a case-sensitive filesystem. Found by QA on 2026-08-16 (`src/store.rs:407-419`).

## Fix

Only treat `user@host:path` as scp form when the text before `@` has no `/`; do not lowercase a local path.
