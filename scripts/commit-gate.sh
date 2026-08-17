#!/bin/sh
# The commit gate. A commit needs formatted code and a clean clippy:
# zero warnings, zero errors. The push gate then holds tests, the
# missouri suite, and the fresh-eyes review.
set -e

# The checks read the working tree. When the index and the tree differ
# on Rust files, the tree check would certify content the commit does
# not contain, so the gate refuses that state first.
if ! git diff --quiet -- '*.rs' 2>/dev/null; then
  echo "commit-gate: unstaged Rust changes differ from the index." >&2
  echo "  Stage them or stash them, so the checked tree is the committed tree." >&2
  exit 1
fi

echo "commit-gate: cargo fmt --check"
cargo fmt --check >/dev/null || {
  echo "commit-gate: the tree is not formatted. Run: cargo fmt" >&2
  exit 1
}

# The exit code is authoritative: -D warnings turns every warning into
# a failure, so no output parsing decides anything.
echo "commit-gate: cargo clippy"
out=$(cargo clippy --workspace --all-targets --all-features --quiet -- -D warnings 2>&1) || {
  echo "commit-gate: clippy is not clean. Zero warnings is the bar." >&2
  printf '%s\n' "$out" | tail -30 >&2
  exit 1
}
echo "commit-gate: ok"
