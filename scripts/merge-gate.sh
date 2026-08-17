#!/bin/sh
# The merge gate. These repos merge by direct push, so this pre-push
# hook is the merge check. A push needs green tests, a green missouri
# suite, and a recorded fresh-eyes review.
#
# The review record is a git note on the pushed tip:
#   git notes --ref=reviews add -m "fresh-eyes: <who> <scope>" <sha>
# Write a note only after an independent reviewer has read the change
# and its test coverage. A note without a review makes the gate false.
#
# The gate has four known limits. The suites test the working tree,
# not the pushed commit. A fresh clone has no hooks until `gaff init
# --git` runs; `gaff check` reports that state. The note checks are
# claim checks: the gate cannot verify a review happened, or that a
# mutation was applied. A note reading "mutations: none" passes. What
# the check changes is what a reviewer writing in good faith thinks to
# do before writing the note. And a merge queue or a direct push by
# GitHub runs no pre-push hook, so CI on the push event is what gates
# those.
set -e

# git sends the ref list on stdin. The first reader spends the stream.
# Capture it before any other program can read it. If a test runner
# read stdin first, the loop below would see EOF and check nothing.
gate_refs=$(cat)

# tests/merge_gate_guard.rs runs this script to cover the note branch.
# Without an escape it would call cargo test from inside cargo test.
# The escape needs both the marker and CARGO, which only a cargo-run
# process sets, so a plain shell cannot turn the tests off with one
# variable. A pushing developer never has CARGO set.
if [ -z "${MERGE_GATE_SKIP_TESTS:-}" ] || [ -z "${CARGO:-}" ]; then
    echo "merge-gate: cargo test"
    # --all-features, because a feature that is off by default is still
    # shipped code. The gate once built without mcp and never compiled it.
    # Capture the output. On red, the failing test's name is the first
    # thing a reader needs, and /dev/null once hid it from the CI log.
    test_out=$(cargo test --workspace --all-features --quiet 2>&1 </dev/null) || {
        echo "merge-gate: cargo test failed. Nothing merges on red tests." >&2
        printf '%s\n' "$test_out" | tail -40 >&2
        exit 1
    }
fi

# The CI runner has no nix, but it preinstalls the packages the
# suites declare. When CI is set, missouri uses the preinstalled
# backend. A local run keeps the nix backend.
if [ -n "${CI:-}" ]; then
    MISSOURI_SANDBOX=preinstalled
    export MISSOURI_SANDBOX
fi

if [ -d tests/missouri ] && { [ -z "${MERGE_GATE_SKIP_TESTS:-}" ] || [ -z "${CARGO:-}" ]; }; then
    command -v missouri >/dev/null || {
        echo "merge-gate: missouri is not on PATH and tests/missouri exists." >&2
        exit 1
    }
    echo "merge-gate: missouri run"
    out=$(cd tests/missouri && missouri run </dev/null 2>&1) || {
        echo "merge-gate: the missouri suite failed. Nothing merges on a red suite." >&2
        printf '%s\n' "$out" | tail -20 >&2
        exit 1
    }
    # The exit code decides. The summary check adds a second gate: the
    # run must show one or more passed paths and zero failures. An empty
    # suite does not pass.
    printf '%s\n' "$out" | grep -E '[1-9][0-9]* passed, 0 failed' >&2 || {
        echo "merge-gate: the suite reported no passing path. An empty suite gates nothing." >&2
        exit 1
    }
fi

# A pull request event checks out a merge commit GitHub creates. No
# reviewer saw that commit, so the loop below would refuse every pull
# request. Check the branch head instead, which is what a reviewer
# read. Both variables come from the runner, so a local shell that
# sets one still meets the loop below.
if [ "${GITHUB_ACTIONS:-}" = "true" ] && [ "${GITHUB_EVENT_NAME:-}" = "pull_request" ]; then
    command -v jq >/dev/null || {
        echo "merge-gate: jq is absent, so the pull request head sha cannot be read." >&2
        exit 1
    }
    head_sha=$(jq -r '.pull_request.head.sha // empty' "${GITHUB_EVENT_PATH:-/dev/null}")
    case "$head_sha" in
    [0-9a-f]*) ;;
    *)
        echo "merge-gate: pull request head sha unreadable. Refusing." >&2
        exit 1
        ;;
    esac
    # No fetch here. A forced fetch of the notes ref discards a local
    # note that no push carries yet, and a gate must not write to the
    # repository it checks. The workflow fetches notes in its own step
    # before this runs.
    gate_refs="refs/heads/pr $head_sha refs/heads/main 0000000000000000000000000000000000000000"
    echo "merge-gate: pull request. Reading the review note on $head_sha."
fi

# Each pushed tip needs a review note. For an annotated tag, the note
# may sit on the commit the tag peels to. A branch deletion merges
# nothing, so it is exempt. The notes-ref exemption keys on the remote
# ref: a push of the reviews ref shares a review record, but a notes
# object pushed at a branch lands on that branch, so that push is
# gated.
zero=0000000000000000000000000000000000000000
printf '%s\n' "$gate_refs" | while read -r _local_ref local_sha remote_ref _remote_sha; do
    [ -z "$local_sha" ] && continue
    [ "$local_sha" = "$zero" ] && continue
    case "$remote_ref" in refs/notes/*) continue ;; esac
    commit_sha=$(git rev-parse --quiet --verify "$local_sha^{commit}" || echo "$local_sha")
    # `|| true`, or set -e ends the loop's subshell on a missing note
    # before either message prints, and the person who forgot the note
    # gets 'failed to push some refs' and nothing else.
    note=$(git notes --ref=reviews show "$commit_sha" 2>/dev/null || true)
    if ! printf '%s' "$note" | grep -q "fresh-eyes"; then
        echo "merge-gate: no fresh-eyes review note on $commit_sha (pushing to $remote_ref)." >&2
        echo "  A reviewer who did not write the change reads it and its test" >&2
        echo "  coverage first. Then record it:" >&2
        echo "    git notes --ref=reviews add -m 'fresh-eyes: <reviewer> <scope>' $commit_sha" >&2
        exit 1
    fi
    # A review that read the change is not enough. Every regression that
    # reached a reviewed tip on 2026-08-16 shipped with green tests and a
    # fresh-eyes note; every one was caught only when the reviewer put
    # the bug back and watched a named test go red. A note that does not
    # say a mutation was applied describes a reading, not a verification.
    if ! printf '%s' "$note" | grep -qi "mutation"; then
        echo "merge-gate: the review note on $commit_sha does not mention a mutation." >&2
        echo "  A test for a guard (a new if, a new early return, a new type check)" >&2
        echo "  is verified by removing the guard and seeing the test go red. Say" >&2
        echo "  in the note which mutations were applied and which test caught" >&2
        echo "  each. A note that only says the change was read is not a review" >&2
        echo "  of its tests. Amend the note in place:" >&2
        echo "    git notes --ref=reviews add -f -m 'fresh-eyes: <reviewer> <scope>. Mutation: <what> -> <test> red' $commit_sha" >&2
        exit 1
    fi
done
echo "merge-gate: ok"
