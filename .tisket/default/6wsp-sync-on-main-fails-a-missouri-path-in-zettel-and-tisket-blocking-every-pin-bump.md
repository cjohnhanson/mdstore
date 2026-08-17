---
title: sync on main fails a missouri path in zettel and tisket, blocking every pin bump
status: todo
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-17T20:01:34Z
updated: 2026-08-17T20:01:34Z
---

Two consumers fail one missouri path each when they build against mdstore main. Neither can bump its pin until this closes.

## Reproduction

Build unmodified zettel main and unmodified tisket main against unmodified mdstore main, through a path dependency. No consumer change is involved.

- zettel: `empty → initialized → has-remote-store → has-synced-store → has-override` fails at the assertion "a branch deleted upstream is pruned on the next sync". 25 passed, 1 failed.
- tisket: `empty → initialized → has-remote-tracker → has-synced-tracker` fails. 29 passed, 1 failed.
- almanac passes: 9 passed, 0 failed.

Each consumer passes its whole suite against its current pin, so the failure arrives with the version jump.

## Suspected cause

`0ff1d22 fix: sync verifies the pin it was told to sync` is the only commit between the pins and main that changes sync behavior. zettel pins 376040b, tisket pins e57ff8d, almanac pins bf12ff1, and mdstore main is 14d08ca. Unverified: no bisect ran.

## Why it blocks

The zettel assertion expects a branch deleted upstream to be pruned on the next sync, and the store row to read `unavailable`. If the new sync semantics are correct, both consumers' fixtures need rewriting to the intended behavior. If the fixtures are right, sync regressed. Deciding which comes first.

## Found by

Work on ccgh, which needs all three consumers to move their pins.
