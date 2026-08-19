#!/bin/sh
# The merge gate. These repos merge by direct push, so this pre-push
# hook is the merge check. A push needs green tests, a green missouri
# suite, and a sign-off for every review .gaff/gaff.yml declares.
#
# The review record is a git note on the pushed tip:
#   git notes --ref=reviews add -m 'signoff[<review>] PASS <sha> <evidence>' <sha>
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

# A change is judged by the policy already in force, not by the policy
# it ships. Without this, a branch that empties `reviews:` passes its
# own check with no sign-off, because the gate reads the file from the
# branch it is judging. Comparing against the merge base closes that.
# A branch that adds a review is fine; one that drops a review is not,
# and the drop lands only once a change carrying it has been reviewed
# under the older, stricter policy.
base_policy=$(git show origin/main:.gaff/gaff.yml 2>/dev/null || true)
if [ -n "$base_policy" ]; then
  base_names=$(printf '%s\n' "$base_policy" |
    awk '/^reviews:/ {inside=1; next} inside && /^  - / {print $2; next} inside && !/^  - / {exit}')
  for name in $base_names; do
    # Only a review the merge base could actually perform is protected.
    # A name with no criteria on the base was never enforceable, so
    # replacing it is not a weakening.
    git cat-file -e "origin/main:.agents/skills/$name/SKILL.md" 2>/dev/null || continue
    if ! gaff reviews | grep -qx "$name"; then
      echo "merge-gate: $name is required on origin/main and this branch drops it." >&2
      echo "  A branch cannot weaken the policy that judges it. Restore the name," >&2
      echo "  or land the removal through a change reviewed under the current policy." >&2
      exit 1
    fi
  done
fi

# Every required review needs vendored criteria, and every vendored
# review needs to be required. A name with no criteria is a review
# nobody can perform. A criterion nobody requires is a check that one
# edit dropped. Checking both directions is what stops that edit.
required=$(gaff reviews)
for name in $required; do
  if [ ! -f ".agents/skills/$name/SKILL.md" ]; then
    echo "merge-gate: $name is required and has no criteria in .agents/skills." >&2
    echo "  Vendor it: almanac add github:cjohnhanson/skills --path skills/$name --name $name --accept" >&2
    exit 1
  fi
done
for dir in .agents/skills/review-*/; do
  [ -d "$dir" ] || continue
  name=${dir#.agents/skills/}
  name=${name%/}
  if ! printf '%s\n' "$required" | grep -qx "$name"; then
    echo "merge-gate: $name is vendored and required by nothing." >&2
    echo "  Name it under reviews: in .gaff/gaff.yml, or remove it." >&2
    exit 1
  fi
done


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

# `gaff reviews check` holds the note requirement, reading the names
# .gaff/gaff.yml declares, so no review name appears here. It carries
# the exemptions too: a branch deletion merges nothing, a push of the
# notes ref shares a record, and an annotated tag peels to its commit.
printf '%s\n' "$gate_refs" | gaff reviews check
echo "merge-gate: ok"
