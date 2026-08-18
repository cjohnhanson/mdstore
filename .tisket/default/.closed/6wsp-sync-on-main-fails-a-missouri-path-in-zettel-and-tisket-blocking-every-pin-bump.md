---
title: sync on main fails a missouri path in zettel and tisket, blocking every pin bump
status: done
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-17T20:01:34Z
updated: 2026-08-17T22:27:46Z
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

## Scratch Notes

## Diagnosed, verified by mutation, closing (2026-08-17)

Cause: mdstore's sync_source resolves the declared pin after a fetch. A pin deleted upstream now fails the sync, where it once reported success and failed later on read as a gix-internal message. The consumers' fixtures still expected exit 0. Not a consumer defect and not a regression; the fixtures encoded the older contract.

The fix is on the fixture side, and the justification is that the suite already sets this contract elsewhere: it establishes that sync refuses a source it cannot reach, in its ssh assertion. A vanished pin matching that contract is consistent rather than convenient. That reasoning is why a test changed instead of the code it tests.

Assertion, as named after a later revision moved the branch deletion inside it:
  a fetch prunes a branch deleted upstream, and the sync then refuses

Mutation: mdstore's sync_source altered so a pin that fails to resolve returns Ok(()) instead of the error. tisket and zettel rebuilt against that mdstore, both suites re-run.

Verdict: caught. tisket 29 passed and 1 failed, zettel 25 passed and 1 failed, and in each the single failure is that named assertion. Unmutated, both suites are green on missouri's own exit code: tisket 30 of 30, zettel 26 of 26.

Two later changes from a cold review, both worth recording. The branch deletion moved inside the assertion, so one invocation proves the whole chain instead of refusing on a prune a neighbouring assertion had performed. And the message is matched by its two parts, pin and side, rather than as one string, which is how mdstore's own test asserts it, so a reworded message now fails in mdstore first.

Work done by the release session, which owned the bisect. Recorded here because the record belongs next to 0ff1d22, the commit the diagnosis is about.
