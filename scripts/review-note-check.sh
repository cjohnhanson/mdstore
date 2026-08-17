#!/bin/sh
# Checks that every pushed tip carries a fresh-eyes review note.
#
# Reads git's pre-push ref lines on stdin. Runs no test suite and
# invokes no build tool, so a test can call it directly.
#
# The note records a review:
#   git notes --ref=reviews add -m "fresh-eyes: <who> <scope>" <sha>
# Write one only after a reviewer who did not write the change reads it
# and its test coverage.
#
# Three limits. The checks are claim checks: this script cannot verify
# a review happened, or that a mutation was applied, so a note reading
# "mutations: none" passes. A fresh clone runs no hook until `gaff init
# --git` installs one. And a merge queue or a direct push made by
# GitHub runs no pre-push hook at all, so CI on the push event gates
# those.
set -e

refs=$(cat)

# A pull request event checks out a merge commit GitHub creates. No
# reviewer saw that commit, so checking it would refuse every pull
# request. Read the branch head instead, which is what a reviewer read.
# Both variables come from the runner, so a local shell that sets one
# still faces the check below.
if [ "${GITHUB_ACTIONS:-}" = "true" ] && [ "${GITHUB_EVENT_NAME:-}" = "pull_request" ]; then
    command -v jq >/dev/null || {
        echo "review-note: jq is absent, so the pull request head sha cannot be read." >&2
        exit 1
    }
    head_sha=$(jq -r '.pull_request.head.sha // empty' "${GITHUB_EVENT_PATH:-/dev/null}")
    case "$head_sha" in
    [0-9a-f]*) ;;
    *)
        echo "review-note: pull request head sha unreadable. Refusing." >&2
        exit 1
        ;;
    esac
    # No fetch here. A forced fetch of the notes ref discards a local
    # note that no push carries yet, and a check must not write to the
    # repository it reads. The workflow fetches notes in its own step.
    refs="refs/heads/pr $head_sha refs/heads/main 0000000000000000000000000000000000000000"
    echo "review-note: pull request. Reading the note on $head_sha."
fi

# For an annotated tag, the note may sit on the commit the tag peels
# to. A branch deletion merges nothing, so it is exempt. The notes-ref
# exemption keys on the remote ref: a push of the reviews ref shares a
# review record, but a notes object pushed at a branch lands on that
# branch, so that push is checked.
zero=0000000000000000000000000000000000000000
printf '%s\n' "$refs" | while read -r _local_ref local_sha remote_ref _remote_sha; do
    [ -z "$local_sha" ] && continue
    [ "$local_sha" = "$zero" ] && continue
    case "$remote_ref" in refs/notes/*) continue ;; esac
    commit_sha=$(git rev-parse --quiet --verify "$local_sha^{commit}" || echo "$local_sha")
    # `|| true`, or set -e ends the loop's subshell on a missing note
    # before either message prints, and the person who forgot the note
    # gets 'failed to push some refs' and nothing else.
    note=$(git notes --ref=reviews show "$commit_sha" 2>/dev/null || true)
    if ! printf '%s' "$note" | grep -q "fresh-eyes"; then
        echo "review-note: no fresh-eyes review note on $commit_sha (pushing to $remote_ref)." >&2
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
        echo "review-note: the note on $commit_sha does not mention a mutation." >&2
        echo "  A test for a guard (a new if, a new early return, a new type check)" >&2
        echo "  is verified by removing the guard and seeing the test go red. Say" >&2
        echo "  in the note which mutations were applied and which test caught" >&2
        echo "  each. A note that only says the change was read is not a review" >&2
        echo "  of its tests. Amend the note in place:" >&2
        echo "    git notes --ref=reviews add -f -m 'fresh-eyes: <reviewer> <scope>. Mutation: <what> -> <test> red' $commit_sha" >&2
        exit 1
    fi
done
echo "review-note: ok"
