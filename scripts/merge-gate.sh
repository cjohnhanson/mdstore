#!/bin/sh
# The merge gate. These repos merge by direct push, so this pre-push
# hook is the merge check: nothing reaches the remote without green
# tests, a green missouri suite, and a recorded fresh-eyes review.
#
# The review record is a git note on the pushed tip:
#   git notes --ref=reviews add -m "fresh-eyes: <who> <what was reviewed>" <sha>
# A note is written only after an independent reviewer has read the
# change and its test coverage. Writing one without a review defeats
# the gate and the point.
#
# Two known limits, stated rather than hidden: the suites run against
# the working tree, not the pushed commit, so pushing a ref other than
# the current checkout is gated by the wrong code; and a fresh clone
# has no hooks until `gaff init --git` runs, which `gaff check` reports.
set -e

# git hands this hook its ref list on stdin, and a stream is spent by
# its first reader. Capture it before anything else can read it: a test
# runner that touched stdin would drain the list, the loop below would
# see EOF, and the gate would pass with nothing checked.
gate_refs=$(cat)

echo "merge-gate: cargo test"
cargo test --workspace --quiet >/dev/null </dev/null || {
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
  # Belt and braces on top of the exit code: the summary must say a
  # nonzero pass count and zero failures, so an empty suite cannot
  # pass by vacuity.
  printf '%s\n' "$out" | grep -E '[1-9][0-9]* passed, 0 failed' >&2 || {
    echo "merge-gate: the suite reported no passing path. An empty suite gates nothing." >&2
    exit 1
  }
fi

# Every pushed tip needs a review note. The note may sit on the commit
# an annotated tag peels to. Branch deletions merge nothing. The
# exemption for notes refs keys on the REMOTE ref: pushing the reviews
# ref itself is how a review record is shared, but a notes object
# pushed AT a branch would land on that branch and must be gated.
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
