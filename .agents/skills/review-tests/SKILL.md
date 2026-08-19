---
name: review-tests
description: >
  Judge test coverage and design. Use as a required review, or as
  /review-tests. Not for judging the code: review-code covers that.
license: MIT
user-invocable: true
---

# Review: tests

You are one independent check on a change you did not write. Judge the
tests. Read `signoff-driver` for the sign-off line, the two severity
bands, and the not-applicable case.

## Method

Mutation testing carries most of this skill. Remove a guard, run the
suite, and name the test that fails. A guard no test kills is untested,
whatever the coverage report says.

Two additions. Judge coverage against the change's goal, not its diff.
Read a state-machine suite as a design, not a score.

If `testing-strategy` is installed, load it. Its mutation framework is
this method, and its coverage pass covers criterion 2. This skill differs
in making mutation mandatory for every new guard rather than periodic. Map
its bands as `signoff-driver` states. If `testing-philosophy` is
installed, load it for the QA-first stance.

## Criteria

1. **Anchor on the goal.** Find the goal the change serves: its issue,
   its commit message, or its pull-request body. Judge coverage against
   that goal. A diff-anchored review passes a change that is fully tested
   and half-built. If no goal is recorded anywhere, say so in the evidence
   and judge against the commit message. **Minor** when a goal exists and
   coverage misses part of it that no user reaches;
   **blocking** otherwise.

2. **Map behavior to tests.** List every behavior the change adds or
   alters. Map each to a named test. A behavior with no test is
   **blocking** when its failure would be silent, and **minor** when a
   later layer catches it. Name the missing case either way.

3. **Kill each guard.** A guard is a new `if`, an early return, a
   bounds check, or a type check. Remove one, run the suite, and name the
   test that fails. A guard no test kills is **blocking**. This catches
   green tests over an unkilled guard: the suite passes whether the guard
   is there or not. Removing a guard may stop the code from running at
   all. Weaken it instead, so the code still runs and the behavior
   changes.

4. **Grade a state-machine suite as a design.** Some repositories test
   through a suite of named states and transitions rather than unit tests.
   Where one exists, ask four questions. Do states name real conditions,
   or numbered steps? Do transitions assert on behavior, or only on output
   shape? Does each path start from a clean state? Does any path depend on
   another running first? A suite that passes and answers these badly is
   **blocking** for the second and third questions and **minor** for the
   others.

5. **Find the vacuous assertion.** An assertion failing for a reason
   other than the one it names passes for the wrong reason. It is
   **blocking** over a behavior this change adds, and **minor**
   elsewhere. The common shape is a negated command that errors early: `!
   tool --flag` passes when `--flag` does not exist. Run each new
   assertion against a deliberately broken input and confirm it fails for
   its stated reason. A vacuous assertion over a new behavior is
   **blocking**.

6. **Name the residual risk.** Say what the suite cannot see. Rank
   what you name by how often the path runs and whether its failure is
   silent. This is **minor** by itself. It becomes the evidence for
   criterion 2 when a named risk has no test at all.

A change may add more guards than the suite's runtime allows. Say so,
state the suite's measured runtime, and sample.

Sample by whether a failure would be silent, never by whether a user
reaches the guard. The guards a user never reaches are the error paths,
the bounds checks, and the permission checks, and those are exactly the
ones whose failure passes unnoticed. Mutate every guard whose removal
would produce a wrong result rather than a crash. Name the guards you left
and why.

## How to run the suite

Find the command the repository documents. Look in its contributing guide,
its continuous-integration workflow, and its build manifest, in that
order. If no command is discoverable, that is a **blocking** finding on
its own: a suite nobody can run is not coverage.

## Worked examples

### A guard no test kills

A change adds a uid boundary to a directory walk, so the walk stops at a
directory another user owns.

A weak review reads the diff, sees `walk_up_stops_at_foreign_owner` in the
test file, and passes.

This skill removes the uid check and runs the suite, which stays green:
the fixture never creates a foreign-owned directory, so the boundary is
untested.

    signoff[review-tests] FAIL 4f1c2ab blocking: removing the uid
    boundary leaves the suite green; the fixture owns every directory

### The same change, done right

The fixture creates a directory owned by another uid, and the boundary
test fails when the check is removed.

    signoff[review-tests] PASS 9d0e1f2 removed the uid boundary and
    walk_up_stops_at_foreign_owner went red; all four new behaviors map
    to named tests; minor: the fixture skips when run as root, and the
    skip is silent
