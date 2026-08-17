---
title: store sync reports synced for a pin whose rev does not exist
status: done
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-16T21:02:18Z
updated: 2026-08-17T16:11:21Z
---

## Problem

`fetch` does not check the declared `rev`; a bad pin surfaces only on read. An unborn or empty source yields a gix-internal message ("delegate.peel_until ... Could not peel 'HEAD'"). Found by QA on 2026-08-16.

## Fix

After a fetch, resolve each consumer's pinned rev and report "pin X not found in <url>" from sync; map the unborn-HEAD error to "the source has no commits".

## Scratch Notes

## 2026-08-17 — fix in review

fix/sync-verifies-the-pin, one commit on mdstore main. Both tests
written first and red against the old behaviour with 'reported
synced'; the fetch-only mutation turns both red, verified after a
first mutation attempt silently failed to compile (string index hit
the wrong Blob occurrence — full output read this time). 201 tests.
Reviewer is asked to attack the None arm's no-commits reasoning, the
fake-oid fixture, the force-push case, and the consumers' sync loops
for an abort-class regression.
Closed at mdstore 3709ed7, landed through the new check-gated path:
branch pushed, gate run green on the sha (one outage rerun, codeload
429 then 503 before any code ran), sha pushed to main, ancestry
verified. The review took one message fix: the no-commits arm now
leads with HEAD not resolving and names both causes, after the
reviewer built a HEAD naming a deleted branch over a source that had
commits.
