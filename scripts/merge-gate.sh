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
set -e

echo "merge-gate: cargo test"
cargo test --workspace --quiet >/dev/null || {
  echo "merge-gate: cargo test failed. Nothing merges on red tests." >&2
  exit 1
}

if [ -d tests/missouri ]; then
  command -v missouri >/dev/null || {
    echo "merge-gate: missouri is not on PATH and tests/missouri exists." >&2
    exit 1
  }
  echo "merge-gate: missouri run"
  (cd tests/missouri && missouri run >/tmp/merge-gate-missouri.$$ 2>&1) || {
    echo "merge-gate: the missouri suite failed. Nothing merges on a red suite." >&2
    tail -20 /tmp/merge-gate-missouri.$$ >&2
    rm -f /tmp/merge-gate-missouri.$$
    exit 1
  }
  grep -E '[0-9]+ passed, 0 failed' /tmp/merge-gate-missouri.$$ >&2 || true
  rm -f /tmp/merge-gate-missouri.$$
fi

# Every pushed tip needs a review note. Branch deletions (zero sha) and
# notes refs are exempt: deleting a branch merges nothing, and pushing
# the notes ref is how a review record itself gets shared.
zero=0000000000000000000000000000000000000000
while read -r _local_ref local_sha _remote_ref _remote_sha; do
  [ "$local_sha" = "$zero" ] && continue
  case "$_local_ref" in refs/notes/*) continue ;; esac
  if ! git notes --ref=reviews show "$local_sha" 2>/dev/null | grep -q "fresh-eyes"; then
    echo "merge-gate: no fresh-eyes review note on $local_sha." >&2
    echo "  A reviewer who did not write the change reads it and its test" >&2
    echo "  coverage first. Then record it:" >&2
    echo "    git notes --ref=reviews add -m 'fresh-eyes: <reviewer> <scope>' $local_sha" >&2
    exit 1
  fi
done
echo "merge-gate: ok"
