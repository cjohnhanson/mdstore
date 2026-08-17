#!/bin/sh
# The merge gate. These repos merge by direct push, so this pre-push
# hook is the merge check. A push needs green tests, a green missouri
# suite, and a review note naming every review the config declares.
#
# `gaff reviews check` holds the note requirement. It reads the pushed
# refs on stdin, and .gaff/gaff.yml states which reviews a change must
# pass, so no review name appears here.
#
# Two limits worth stating. The suites test the working tree, not the
# pushed commit. And a pre-push hook is advisory: `--no-verify` skips
# it, so the required CI check is what actually enforces this.
set -e

# git sends the ref list on stdin. The first reader spends the stream.
# Capture it before any other program can read it. If a test runner
# read stdin first, the check below would see EOF and check nothing.
gate_refs=$(cat)

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

# The CI runner has no nix, but it preinstalls the packages the
# suites declare. When CI is set, missouri uses the preinstalled
# backend. A local run keeps the nix backend.
if [ -n "${CI:-}" ]; then
	MISSOURI_SANDBOX=preinstalled
	export MISSOURI_SANDBOX
fi

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

command -v gaff >/dev/null || {
	echo "merge-gate: gaff is not on PATH, so the review check cannot run." >&2
	echo "  cargo install --git https://github.com/cjohnhanson/gaff" >&2
	exit 1
}

# Last in the pipeline, always. A POSIX pipeline exits with its final
# command, so anything after this would discard the refusal.
printf '%s\n' "$gate_refs" | gaff reviews check
echo "merge-gate: ok"
