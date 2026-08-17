---
title: The gate cannot pass on a pull request
status: todo
priority: '1'
assignee: null
due_date: null
labels:
- ci
- blocker
depends_on: []
created: 2026-08-17T16:53:57Z
updated: 2026-08-17T16:53:57Z
---

Branch protection on main requires the check named `gate`. A pull request can never turn it green, so no pull request can merge.

## Why

`gaff ci` synthesizes the pre-push ref line from HEAD (gaff src/cli.rs, run_ci). On a `pull_request` event, `actions/checkout` leaves HEAD at the ephemeral merge commit GitHub creates for the pull request.

`scripts/merge-gate.sh` then requires a review note on that commit. The merge commit did not exist before the pull request opened, so no note can be attached to it. The notes exemption keys on `refs/notes/*`, and the synthesized ref is never that, so the check runs and fails.

For a fork pull request the notes fetch reads the base repository only, so an outside contributor cannot attach a note at all.

## Before this

The workflow triggered on `push:` only. The required check never reported, so a pull request blocked silently. Adding `pull_request` made it report, and report red. Visible beats silent, so the trigger stays.

## Options, none decided

1. Run only the commit gate and the tests on a pull request. The review note gates the push to main, which is where a merge lands. Needs a hook selector the gaff action does not expose, or `run: false` plus explicit steps.
2. Make merge-gate.sh read the note from the pull request head sha rather than checked-out HEAD.
3. Exempt a synthesized detached ref the way `refs/notes/*` is exempted.

Option 1 matches the model: a note records that a reviewer read a change, and a pull request is a proposal rather than a merge.

## Scope

All six repositories. Every one requires `gate` and every gate workflow had the same push-only trigger.

CONTRIBUTING.md states the limitation and tells a contributor to open the pull request anyway.

## Scratch Notes

## Confirmed in production 2026-08-17

PR #16 proves it. Two runs on the same tip:

- push event: **pass**, 30s. Checked out 5ea220e, which carries a review note.
- pull_request event: **fail**, 33s.

The failure line:

    merge-gate: no fresh-eyes review note on 57cf6136d4585fdb4067fedb10e65182a994e61f (pushing to refs/heads/detached)

57cf6136 is not a commit on the branch. It is the merge commit GitHub creates for the pull request. The synthesized ref is refs/heads/detached, which gaff builds for a detached HEAD, so the refs/notes/* exemption does not fire.

Pushing refs/notes/reviews to the remote does not help. The note exists on 5ea220e. No note can exist on a merge commit GitHub creates after the fact.

So the reasoning from gaff src/cli.rs run_ci holds, and observation matches it.
