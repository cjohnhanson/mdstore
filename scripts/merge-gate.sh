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
# The gate has two known limits. The suites test the working tree, not
# the pushed commit. A fresh clone has no hooks until `gaff init --git`
# runs; `gaff check` reports that state.
set -e

# git sends the ref list on stdin. The first reader spends the stream.
# Capture it before any other program can read it. If a test runner
# read stdin first, the loop below would see EOF and check nothing.
gate_refs=$(cat)

echo "merge-gate: cargo test"
# --all-features, because a feature that is off by default is still
# shipped code. The gate once built without mcp and never compiled it.
cargo test --workspace --all-features --quiet >/dev/null </dev/null || {
  echo "merge-gate: cargo test failed. Nothing merges on red tests." >&2
  exit 1
}

if [ -d tests/missouri ]; then
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
  if ! git notes --ref=reviews show "$commit_sha" 2>/dev/null | grep -q "fresh-eyes"; then
    echo "merge-gate: no fresh-eyes review note on $commit_sha (pushing to $remote_ref)." >&2
    echo "  A reviewer who did not write the change reads it and its test" >&2
    echo "  coverage first. Then record it:" >&2
    echo "    git notes --ref=reviews add -m 'fresh-eyes: <reviewer> <scope>' $commit_sha" >&2
    exit 1
  fi
done
echo "merge-gate: ok"
