---
title: Confine store filesystem access with a capability handle
status: todo
priority: null
assignee: null
due_date: null
labels:
- security
- architecture
depends_on: []
created: 2026-08-16T23:05:29Z
updated: 2026-08-16T23:05:29Z
---

## Scratch Notes

# QA plan: confined store access

Written before any code. Each scenario becomes a test. Plain language
first, so a gap is visible while it is still cheap.

## What changes

`StoreContent::Dir` joins caller text onto a real directory. That is
the whole exposed surface. Four rounds of review found a different
unguarded call site each time, so the guard moves from a predicate a
caller must remember into a capability the caller cannot escape.

A `StoreDir` holds an open directory handle for the store root. Every
read, write, and scan goes through the handle. An escape fails at the
syscall.

`StoreContent::GitTree` reads store content from the git object
database. No filesystem path is built from third-party text there, so
it is already confined and does not change. The bare-clone cache is a
location mdstore chooses, read by gix with ambient authority, and does
not change either.

## Escape through a document name

1. A note id of `../../outside` reaches nothing outside the store.
2. An absolute id of `/etc/passwd` reaches nothing.
3. An id of `..` alone, and of `.` alone.
4. An id holding a NUL byte.
5. A nested climb: `a/b/../../../../outside`.
6. A Windows-style climb on a Unix host: `..\..\outside`.
7. A Unicode separator lookalike: U+2215 division slash.
8. A percent-encoded climb: `%2e%2e%2f`.
9. A very long name, past the platform limit.

## Escape through the filesystem

10. The document directory is a link pointing outside the store.
11. A `.md` file inside the store is a link pointing outside.
12. A subdirectory is a link pointing outside. Covers a tisket project
    and an almanac skill directory.
13. `stores.yml` is a link.
14. The staging path for a write is a link planted in advance.
15. A hard link to a file outside the store.
16. A FIFO where a document should be.
17. A file replaced by a link between the check and the open.

## Legitimate cases that must keep working

18. Create, read, edit, and delete an ordinary note.
19. A document inside a subdirectory, as tisket projects use.
20. A store root given as a relative path.
21. A store root that is itself a link to a real directory. A person
    may keep a store behind a link, and that must work.
22. A git-backed store, which touches no filesystem for content.
23. Two stores in one closure, one local and one git-backed.
24. A store on a case-insensitive filesystem.

## Failure behaviour

25. The store root does not exist.
26. The store root is a file, not a directory.
27. The store root is unreadable.
28. One document is unreadable. The rest of the store still answers.
29. One document is not UTF-8. The rest still answers.
30. A write fails part way and leaves no partial document and no stray
    temporary file.

## What each layer proves

- Unit tests in mdstore prove the handle refuses each escape.
- Missouri paths prove the tools behave correctly end to end, through
  the CLI and through the MCP server, because a guard that holds in
  the library and is bypassed by a caller is the exact defect four
  rounds kept finding.

## Progress, 2026-08-16

A fresh-eyes review refused the first attempt. Two of its four fixes
did not close their defect and two of its tests were vacuous: one
asserted on helpers the walk never calls, and one passed a value the
walk never produces.

The cause is the class this issue exists to end. A predicate every
caller must remember is a predicate some caller forgets.

Done, on branch fix/confined-store:

- StoreDir holds an open directory for the store root. Every read,
  write, and scan goes through the handle. cap-std supplies it.
- A test proves the capability rather than the flag: an open through
  a link that leaves the store fails even with a plain create.
- on_machine_location asks git::local_path, which asks the parser
  that performs the fetch. unshareable calls the same resolver.
- A local git source carries its resolved location into
  member_identity.
- The location guard has a test that drives StoreGraph::open.

163 unit tests. Every guard is mutation-checked.

Remaining: route the three tools through StoreDir, and missouri
coverage for each scenario in the plan above.

Note: this work touches StoreGraph::open. The codelikecody session
plans a trust parameter there and must sequence after it.
## 2026-08-16 19:33 — round five, after a scratchpad wipe

The session scratchpad was wiped on restart. Worktrees lived there and
were not committed, so two hours went: the StoreContent rewiring in
mdstore, and the whole Repo conversion in zettel. Worktrees now live in
~/Projects/.wt/*-confine, and each step commits.

Fixed from the round-five review, on fix/confined-store:

1. StoreContent::Dir holds the handle, not a PathBuf. The module was
   present and unwired; a read of '../outside/secret.md' returned the
   file. Test a_store_read_cannot_leave_the_store asserts each escape
   the reviewer reproduced, and fails if the ambient join returns.
2. A StoreDir clone shares the open directory. Re-opening per operation
   resolved the root through ambient authority each time, so a swapped
   root was followed, and a 500-document scan cost 501 root opens.
3. write no longer removes a staging file it did not create. The error
   path destroyed another writer's content.
4. Both staging tests were vacuous. The staging name is now a
   parameter, so each test plants at the name the write takes.
5. present_but_irregular moved onto the handle. As ambient
   symlink_metadata it answered for any absolute path.
6. The empty relative path names the root; the OS refuses it. A scan of
   the root read nothing and NotFound hid it as an empty store.
7. The cfg(unix) block and the nix dependency came from another
   session through a file copy. Removed; no dependency is target-gated.

Open, not fixed here:

- The confinement root is still chosen by the text it guards. A
  vendored dependency declaring 'path: ../../secret' gets a handle
  rooted at the secret. Climbing one level is the documented sibling
  pattern, so the fix is a trust boundary, not a path check. The other
  session is building the trust-parameter work on the walk-site guard;
  coordinate rather than duplicate.
- ScanEntry.path changed from absolute to relative. Version is still
  0.2.0 and nothing documents the break.
- is_remote_url still hand-parses schemes beside url::Url and
  gix::url::parse. Three parsers answer one question.

183 tests green, clippy clean, fmt clean.
## 2026-08-16 19:50 — landing

mdstore fix/confined-store tip 55650f3, seven commits ahead of main.
origin/main is still a98abb7, so the tip fast-forwards; landing is one
push once the gate is satisfied.

The gate is scripts/merge-gate.sh as a pre-push hook: cargo test, the
missouri suite with a nonzero pass count, and a fresh-eyes note under
refs/notes/reviews on the pushed tip. No note exists anywhere in the
repo yet. The round-five review read 95408bd, the parent of all four
fixes, so it cannot vouch for this tip.

A cold reviewer is running against an isolated snapshot of the tip, not
the worktree. A live-tree reviewer's mutation reached a commit once
before. No commits go on mdstore until it reports, or the note would
describe a tip that no longer exists.

zettel fix/confined-store is at a569ca4, two commits ahead. Cargo.lock
stays dirty on purpose: it carries a .cargo/config.toml patch at the
unpushed mdstore worktree. Both are excluded from commits. When mdstore
lands, that override is replaced by a real rev pin.

tisket and almanac are unstarted.
## 2026-08-16 21:07 — all four tools migrated, two reviews open

mdstore landed once already: origin/main reached 97ab454 through the
gate with a fresh-eyes note, then the other session rebased its
user-config work on top and main is now 376040b. The rebase kept the
deletions and the 0.3.0 version.

Consumers, all in ~/Projects/.wt/<tool>-confine:

- zettel fix/confined-store, three commits. Repo holds the handle;
  ten document calls, five directory loops, the delete and the check
  walk go through it. 34 tests. Cold review running.
- tisket fix/confined-store, two commits. Repo holds the handle;
  fourteen document calls, seven directory loops, close, reopen, the
  cross-project move and project creation go through it. projects_of
  collapsed onto the store's own listing. 14 tests.
- almanac fix/confined-store, one commit. The reference read was a
  canonicalize and a starts_with on caller text; it is a handle now.
  SKILL.md reads through a handle. subdirectories collapsed onto the
  store, the third copy of that listing. 71 tests.

mdstore fix/confined-moves adds rename, remove_dir and dir_is_empty,
because tisket closes an issue by moving it and had no confined way to.
Round one returned DO-NOT-LAND. The finding that mattered: the escape
assertions passed on ENOTEMPTY rather than on the refusal, so replacing
both methods with ambient std::fs left the whole suite green. The test
now points at an empty directory outside the store and both mutations
die. rename replacing an existing destination is documented and pinned.
Version corrected 0.4.0 to 0.3.1, since the change only adds. Round two
running.

Order to land: mdstore moves, then tisket (needs it), then zettel and
almanac, which do not. The other session's eikj branches rebase after.

Not done: the five follow-ups in issue jeej, and a fresh-eyes review
for tisket and almanac.
## 2026-08-16 21:16 — review round two on every branch

Nothing pushed yet. Four branches, all committed, all in
~/Projects/.wt/<tool>-confine.

mdstore fix/confined-moves, three commits on origin/main. Adds rename,
remove_dir and dir_is_empty. Two rounds of review. Round one caught
escape assertions that passed on ENOTEMPTY rather than on the refusal,
so ambient std::fs mutations survived the whole suite. Round two caught
that the rename rustdoc was never written: the edit script asserted on
a later match, threw, and never wrote, while the commit message said it
had. Both fixed. 197 tests.

zettel fix/confined-store, four commits. Round one found a regression I
introduced: opening the handle in Repo::open made every command fail on
a store whose note directory is absent, which is any clone made before
the first note, because git tracks no empty directory. Also found four
guards with no test, two of which combined to delete a file outside the
store. The root cause of that pair was resolve_id matching an exact id
with Path::exists, which follows a link, so a planted link resolved to
an id that list and scan both refuse to show. Fixed. 39 tests, missouri
25 passed 1078 assertions.

tisket and almanac: first review running.

Recurring failure worth naming: three times today a multi-edit python
script asserted on a later match, threw, and wrote nothing, while the
work carried on as though the edit had landed. Each edit now writes and
is verified separately.

Land order: mdstore moves, then tisket, then zettel and almanac.
## 2026-08-16 21:33 — zettel landed, two consumers left

Landed:
- mdstore origin/main bf12ff1, gate satisfied, review note pushed.
- zettel origin/main c9058ab, gate satisfied including missouri at 25
  paths and 1078 assertions, review note pushed.

In review:
- tisket fix/confined-store, four commits. Round one DO-NOT-LAND on
  three items, all fixed. The one that matters: the escape test's move
  assertion named a source file that was never created, so it failed on
  ENOENT. An ambient std::fs::rename left the whole suite and all of
  missouri green while a move carried an issue out of the tracker. Same
  defect mdstore fixed in its own escape test earlier the same day, and
  the copy here was written after that fix. projects_of had no coverage
  at all; an empty return broke nothing while a tracker's issue count
  read zero in the output a person sees.
- almanac fix/confined-store, two commits. Round one DO-NOT-LAND on
  four items, all fixed. A library shipping 'references -> /etc' served
  /etc/hosts through show, because a handle confines names and does not
  choose its root.
- mdstore fix/confined-symlink-cover, one commit, the two follow-ups
  the moves reviewer named.

Filed: jeej and uq57 in mdstore, ki5w in zettel.

The recurring shape across three repos: an escape assertion that passes
on an unrelated error. ENOTEMPTY in mdstore, ENOENT in tisket, a
non-existent bait file in almanac. Each shipped green with the guard
fully removed. Point the assertion at something that exists and can
only fail for the reason claimed.
## 2026-08-16 21:52 — goal reached

All four landed on main through their own gates, each with a fresh-eyes
note pushed alongside:

  mdstore  fdf3061
  zettel   c9058ab (main since moved; the work is an ancestor)
  tisket   bee9023
  almanac  05d5b60

Verified rather than asserted: zero call sites of read_document,
write_document, is_regular_file and scan_documents across all three
consumers, and every pin at 376040b or later.

Real holes found by review, not test gaps:

- almanac served /etc/hosts through a library shipping
  'references -> /etc'. A handle confines the names used under a root
  and does not choose the root. The predicate it replaced had the same
  hole, so it was inherited rather than introduced.
- almanac then enumerated 61 filenames out of /etc into an agent's
  context, because the listing seven lines below the fixed read path
  still followed the link.
- zettel opened the handle in Repo::open, so every command failed on a
  store whose note directory is absent, which is any clone made before
  the first note.

The recurring test defect, in three repos, two of them mine: an escape
assertion that passes on an unrelated error. ENOTEMPTY in mdstore,
ENOENT in tisket, a bait file planted one directory too high in
almanac. Each shipped green with the guard fully removed. The tisket
one was written after the mdstore fix.

Follow-ups filed: jeej, uq57 and the race issue in mdstore, ki5w in
zettel, niro in tisket, xxba in almanac. The most serious is xxba: a
named pipe in a skill directory hangs almanac and the MCP server
forever, through unguarded ambient reads that predate this work.

Not landed: zettel fix/lazy-note-handle, which holds the handle lazily
so a read stops creating the note directory. Committed work is in the
worktree, unreviewed.
## 2026-08-16 22:47 — the follow-ups are being fixed, not filed

Reversed course after the user objected to filing rather than fixing.
Four branches in review, none landed:

- mdstore fix/store-error-keeps-the-kind (5b2ba2e). A store error now
  carries the errno and exposes refused_by_confinement(). cap-std
  reports an escaping path as PermissionDenied and so does a mode-000
  directory; only the errno separates them, and io_error was throwing
  it away. tisket's fix depends on this. 198 tests.
- tisket fix/load-project-and-closed (3f96858). Round one caught two
  regressions I introduced in the fix itself: swallowing every scan
  failure turned a permissions fault into zero issues with exit 0, and
  mapping a read failure onto ProjectNotFound reintroduced the layer
  split one line below where I closed it. Both fixed. The test's
  fixture held nothing beyond the link, so an ambient scan produced
  the same empty answer; it asserts on the scan itself now.
- almanac fix/no-hang-on-a-pipe (219bb95). Round one: the first fix
  closed two doors of six. status, sync --check, add and update all
  hung on the same pipe one command after check named it. All six
  guarded, mutation-checked.
- zettel fix/lazy-note-handle (8357972). Round one: a comment I wrote
  claimed the anti-swap property was unchanged, and it was not.
  Deferring the open deferred the containment check with it, and a
  five-line probe listed a note from outside the store. The check runs
  where the handle opens now. Also: swallowing every open failure hid
  a mode-000 directory as an empty store; OnceCell cost Repo its Sync.

Also this hour: moved seven tisket issue files out of the shared
checkouts, where every 'cd ~/Projects/<repo> && tisket ...' had been
writing them. Committed on docs/tisket-issues in each worktree, which
is why this scratch is being written from ~/Projects/.wt/mdstore-issues.

Recurring: a multi-edit script asserting late and writing nothing.
Fourth time tonight. One edit per script now, verified by grep.
## 2026-08-16 23:20 — round three across the board

Every branch went through fail-first this round, and it caught me
once: the copy_tree assertion in almanac passed with the guard removed
because the destination sat inside the tree being copied. Red-first
found it in one run.

Landed nothing this hour. Four reviews open:

- mdstore fix/store-error-keeps-the-kind, first review. tisket needs
  it. The reviewer is asked to verify the cap-std errno claim from
  source across every backend, and to say which direction fails safe.
- mdstore chore/gate-requires-mutation-verified, unreviewed. The gate
  refuses a review note that does not mention a mutation. Verified
  against a throwaway repo: no note refused, a read-only note refused,
  a mutation note accepted.
- tisket round three. Round two found the fault half untested and a
  relative in-store link still splitting the layers. Both fixed
  fail-first. Also: I broke the reviewer's build by switching the
  mdstore worktree's branch under them. One worktree per branch now.
- almanac round three. Round two: status called a pipe drift, the
  scanner guard ate the path-only checks, two guards had no test.
  Fixed; status pinned by a missouri transition since it prints.
- zettel round two, running.

Memory: three new entries and one revised. A guard test must fail
first. A fix covers the mechanism. Worktree means every write. One
edit per script, verified by grep on the file.

The peer session attributed missouri commits to me a second time; the
reflog shows every entry is theirs, including the branch rename that
put them there. Sent it.
## 2026-08-17 00:20 — landing in sequence under load

mdstore e57ff8d landed: a store error carries the io kind, Error is
non_exhaustive, refused_by_confinement and io_kind exposed. Reviewer
verified the cap-std errno claim from source across every backend and
found the fail-safe direction runs loud.

Ready to push, in order, one at a time because load average is 130
and each gate takes ten minutes:

1. zettel fix/lazy-note-handle, LAND, pushing now through its gate.
2. tisket fix/load-project-and-closed, LAND, pinned to e57ff8d, note
   written, waiting for zettel to clear.
3. almanac fix/no-hang-on-a-pipe, LAND, note not yet written.
4. mdstore chore/gate-requires-mutation-verified, unreviewed.

Every fix this round was written test-first and each was verified by
putting the bug back. Reviewers found the process failures too: I
switched a worktree branch under a reviewer's build, and I wrote a
copy_tree assertion whose destination sat inside the source so it
passed on the recursion error. Both caught, both fixed.
## 2026-08-17 01:45 — everything landed

On main, each through its gate with a note naming the mutations that
killed each guard:

  mdstore  85ba0bd  error kind carried; Error non_exhaustive; the gate
                    refuses a review note that names no mutation
  zettel   bfbc746  lazy note handle; containment rechecked at the
                    open; a fault is loud
  tisket   85816bf  refusal skipped, fault loud; load_project and
                    list_projects ask the same two questions
  almanac  715ed1d  all six pipe doors guarded; status says unreadable;
                    scanner keeps its path-only checks

Two things the landing taught. A fifteen-minute gate inside a push
does not survive this machine sleeping or GitHub closing the socket;
running the gate locally first, then pushing with a warm target, is
what got the last four through. And two sessions pushing to one main
race: whichever finishes its suite second is rejected as
non-fast-forward and reruns everything. tisket lost that race once.

Every fix in the last two rounds was written to fail first. It caught
one vacuous assertion of mine before a reviewer could, and every
reviewer still found something a mutation I had not imagined exposed.
The gate now requires the note to name what was mutated. It cannot
verify the claim, and says so.

Filed and left, by design: the store-root trust boundary (uq57) and
the check-then-act races no deterministic test catches (v882). Both
need a decision, not a patch.
