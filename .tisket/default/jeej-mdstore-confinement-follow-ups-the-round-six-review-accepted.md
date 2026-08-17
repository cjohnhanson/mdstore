---
title: 'mdstore: confinement follow-ups the round-six review accepted'
status: todo
priority: null
assignee: null
due_date: null
labels:
- review-followup
depends_on: []
created: 2026-08-17T01:44:51Z
updated: 2026-08-17T01:44:51Z
---

The round-six fresh-eyes review returned LAND on 97ab454 and accepted
five items as follow-up. Each is recorded here so the reasoning is not
lost with the review.

1. A hard link inside the store reaches a file outside it. Proved:
   `ln outside/secret.md store/docs/hard.md`, then `read("docs/hard.md")`
   returns the outside content, `is_document` is true, and `scan` lists
   it with no skipped entry. Confinement cannot fix this: a hard link
   has no path to refuse. Exploiting it needs write access inside the
   store on one filesystem, and neither git nor a tarball carries hard
   links. The fix is one line in "What this does not cover" in
   src/confined.rs, which currently promises more than it delivers.

2. Tests in src/confined.rs call std::os::unix::fs::symlink with no
   cfg(unix). src/store.rs gates its equivalents. CI is ubuntu-only, so
   nothing fails today. It breaks the day a Windows runner is added, and
   Cargo.toml declares no platform restriction.

3. subdirectories omits an internal directory symlink that scan and read
   both follow. It fails closed, so a directory is hidden rather than
   exposed. Same class as item 4; settle them together.

4. scan refuses an inside-climb only when the target is missing.
   scan("docs/../absent") errors while scan("absent") is Ok(empty) for
   the same missing directory, and read, is_document and subdirectories
   all permit the climb that scan refuses. It fails closed and the doc
   states the choice, but it is a lexical-versus-capability disagreement
   inside the module built to remove them.

5. Scan.skipped changed from absolute to relative exactly as
   ScanEntry.path did. The changelog documents one and not the other,
   and the field doc names the consumer that prints it.

Two smaller notes from the same review:

- climbs() was inserted between at()'s doc comment and at(), so the doc
  describing at() is attached to climbs() and at() has none.
- The soundness of the NotFound arm rests on cap-std reporting every
  escape as PermissionDenied, which nothing in this repo asserts. A test
  pinning that contract would close it. The current test uses a path
  with '..', so it never exercises the error-kind assumption.

Also raised, larger than the rest:

- mcp::Served holds root: PathBuf, not a StoreDir. The module does no
  filesystem work today. The concern is that it hands the next
  implementer a path and no handle, on the surface that takes document
  names from a network client. The gate now compiles that module, so
  that work happens under green CI with no structural hint.

## Scratch Notes

## 2026-08-17 — items 1 through 5 and both notes are a branch

fix/accepted-followups, in review:

- One climb rule. A '..' component is refused by every operation, as
  StorePath with the errno-less PermissionDenied shape, so
  refused_by_confinement covers a lexical refusal and an OS refusal
  alike. Items 3 and 4 collapse into this; scan's NotFound tiebreaker
  is gone because a climb cannot reach it.
- Module doc names hard links and in-store directory links as
  uncovered, with the consumer-level mitigations (item 1, item 3's
  residue).
- Eight tests gated cfg(unix) (item 2).
- Scan.skipped documents its 0.3.0 break (item 5).
- ServeConfig.root states the open-a-StoreDir contract; the field
  stays a path because a config deserializes (the larger note).
- at() has its doc back (smaller note 1).

Smaller note 2 was already closed by 0.3.2:
a_refusal_and_a_permissions_failure_carry_different_kinds pins the
errno contract directly, on a real escape.

This issue closes when the branch lands.
