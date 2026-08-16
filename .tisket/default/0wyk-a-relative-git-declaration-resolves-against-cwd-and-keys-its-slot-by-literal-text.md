---
title: 'a relative git: declaration resolves against cwd and keys its slot by literal text'
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

`git: ../up` resolves against the process cwd (as the git CLI clone did), and the slot name hashes the literal text, so two roots declaring `../up` share one slot and store identity dedups them. Found by QA on 2026-08-16. Pre-existing; the gix rewrite kept the behavior.

## Fix

Resolve a local `git:` URL against the declaring `stores.yml` before it reaches `git.rs`, and key the slot and the identity by the resolved absolute path. `sync_source` needs the declaring root for that.
