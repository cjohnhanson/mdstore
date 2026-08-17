---
title: the confinement docs claim a kernel refusal that macOS does not provide
status: todo
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-17T22:14:11Z
updated: 2026-08-17T22:14:11Z
---

Two module docs claim the operating system enforces store containment. On macOS it does not; cap-std resolves the path in userspace.

## The claim

`src/confined.rs:13-16` reads: "[`StoreDir`] holds an open directory. Every read, write, and scan goes through the handle, and the operating system refuses a path that leaves it."

## What was measured

cap-primitives 4.0.2 uses `openat2` with `RESOLVE_BENEATH` on Linux, and `O_RESOLVE_BENEATH` on FreeBSD. Every other target routes to `crate::fs::manually`, whose own header states the method: "Manual path resolution, one component at a time, with manual symlink resolution, in order to enforce sandboxing." macOS is in that set, and macOS is the platform this repository is developed on.

So containment on macOS rests on cap-std's userspace resolver, not on a kernel refusal. The guarantee is real and it is a library guarantee.

Found by the fresh-eyes review of the pull request body for ccgh, which checked the same phrasing that this branch had copied into `src/tool.rs`. The copy is corrected on that branch; this issue covers the original.

## Why the wording matters here rather than being a nit

The distinction is the whole argument of the module. A kernel check holds against a race between the check and the open. A userspace resolver walking one component at a time is stronger than a predicate and weaker than `RESOLVE_BENEATH`, and the module's own "What this does not cover" section already lists two gaps that follow from that. A reader who believes the kernel refuses every escape will not look for the third gap.

## Fix

State the mechanism per platform, or state it neutrally: an open directory handle that confines every path beneath the root, with `RESOLVE_BENEATH` on Linux and FreeBSD and cap-std's resolver elsewhere. Do not claim a kernel refusal on a platform that has none.
