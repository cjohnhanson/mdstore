---
title: store sync reports synced for a pin whose rev does not exist
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

`fetch` does not check the declared `rev`; a bad pin surfaces only on read. An unborn or empty source yields a gix-internal message ("delegate.peel_until ... Could not peel 'HEAD'"). Found by QA on 2026-08-16.

## Fix

After a fetch, resolve each consumer's pinned rev and report "pin X not found in <url>" from sync; map the unborn-HEAD error to "the source has no commits".
