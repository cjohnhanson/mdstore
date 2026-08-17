---
title: the user config is named for the library, not the tool
status: todo
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-17T19:32:56Z
updated: 2026-08-17T19:32:56Z
---

Configuring tisket requires knowing that mdstore exists. The user config lives at `~/.config/mdstore/config.yml`, named for the shared library, so a user who has never heard of mdstore cannot find the file that decides where tisket reads. One file also fuses three tools: a root set for zettel is the same root for almanac and tisket.

Shared config logic in the library is correct and stays. Ownership of the user-facing path is the defect.

## End state

Each tool reads its own config. tisket reads `~/.config/tisket/config.yml`, zettel reads `~/.config/zettel/config.yml`, almanac reads `~/.config/almanac/config.yml`. mdstore keeps parsing, the format gate, the passwd-home rule, and the atomic write. mdstore stops naming the directory.

`Vocabulary` already carries `tool` through `resolve_root`, so the name is present at every site that reports the path today.

## Scope

1. `src/userconfig.rs`: `config_path`, `load`, and the fixed-path write take the tool name. The module doc states the shared-file rationale and needs rewriting, not amending.
2. `src/resolve.rs` lines 120 and 133: two error strings name `~/.config/mdstore/config.yml` literally. Both must report the calling tool's path.
3. `src/registry.rs`: `registry.yml` is user-facing config under the same rule. It also resolves through `MDSTORE_REGISTRY` and `XDG_CONFIG_HOME`, which contradicts the doctrine `userconfig.rs` states for itself (no environment channel, passwd home only). Decide one rule for both files.
4. `src/git.rs`: `cache_root` stays `~/.cache/mdstore/stores`. A cache is not config, and one bare clone per URL serves all three tools.
5. Call sites are uniform. Each tool has `load_user_config` in `src/cli.rs` and a `store root` write: zettel 524-527 and 608-610, tisket 574-577 and 655-657, almanac 463-466 and 553.
6. Three repos pin three different mdstore revs: zettel 376040b, tisket e57ff8d, almanac bf12ff1. Each repo gates merges on review sign-off, so three reviewed changes follow the library change.

## Decisions

1. Does an absent per-tool config fall back to `~/.config/mdstore/config.yml`? A silent fallback contradicts the "no silent downgrade to unconfigured" rule the module already states. The alternative errors and names the new path.
2. Does the registry keep environment resolution, or adopt the passwd-home rule that `userconfig.rs` states? One crate holding two opposite rules for two config files is the current state.

## Migration

This machine holds `~/.config/mdstore/config.yml` with `root_store: /Users/codyhanson/Projects/co.d/plaintext`. After the change, three files carry that value, and the same directory stays the root tracker, note store, and skill library.

## Scratch Notes

## Decisions taken (2026-08-17)

1. No fallback to the old shared path. An absent `~/.config/<tool>/config.yml` means no root fallback, which is the benign absence the module already defines. A read of the retired mdstore path would keep the defect alive in code and hide an unmigrated machine. Migration is explicit: write the three per-tool files.

2. registry.yml moves per tool as well. Same defect, same rule: a user hand-editing a URL override should not need to learn the library's name. Environment resolution stays as it is, because tightening it to the passwd-home rule is a separate topic. The doctrine mismatch between the two config files gets its own issue.

Tension recorded, not resolved: one URL-to-checkout map served all three tools, and three maps now hold the same rows. No cross-tool include mechanism exists. If duplication becomes a real cost, the answer is a declared include, not a return to a library-named directory.

3. cache_root stays `~/.cache/mdstore/stores`. A cache is not config.

## Order of work

1. mdstore, on a branch: failing tests first for the per-tool path, then `config_path(tool)`, the write path, both resolve.rs error strings, and registry.rs.
2. Three pin bumps, each passing the tool's own name: zettel, tisket, almanac. Each repo gates on review sign-off.
3. Write `~/.config/{tisket,zettel,almanac}/config.yml`, each with root_store co.d/plaintext, and delete the mdstore one.
4. co.d carries no store config today, and the root pointer exists only on this machine. Whether it becomes repo-managed is a separate decision, deliberately not folded in here.
## State 2026-08-17, end of session

mdstore is done and committed. Branch fix/per-tool-user-config, commits 9c6d993 (issue) and 4082571 (change). 183 unit tests plus 1 integration test pass, clippy clean. The guard test was verified by mutation: restoring the library directory in config_path fails it on its own assertion.

What changed: config_path, UserConfig::load, UserConfig::save_root, registry_path, and Registry::load all take the tool name. Both resolve.rs error strings report the caller's path. cache_root is untouched. README rewritten.

Registry::load is not unused after all. Every consumer calls it in workspace.rs, so the earlier note claiming no callers was wrong; the grep covered only the mdstore repo.

Three consumers are changed but uncommitted, in worktrees under ~/Projects/.wt/<tool>-tool-config, branch fix/per-tool-user-config off each main. Patches are saved in the session scratchpad. Each carries the same shape: a crate-level TOOL const, VOCAB taking tool from it, the three mdstore calls passing it, docs, and missouri fixture text.

One extra fix, found while editing: store root printed a literal config path, so --user-config made the shown path a lie. It now prints the file that was read. almanac needed a redundant clone removed, because the match borrows where it used to move.

Verified against the mdstore branch through a path dependency: zettel 43 tests, tisket 19, almanac 77, each fmt and clippy clean. almanac missouri 9 passed 0 failed.

Blocked, filed as 6wsp: zettel and tisket each fail one remote-sync missouri path when built against mdstore main. Both failures reproduce on unmodified main with no consumer change, so they are pre-existing in the pin gap and not caused by this work. Suspect 0ff1d22, unbisected.

The consumer commits cannot be made yet. Each repo's commit gate runs clippy, and the code does not compile against the old pinned mdstore. The pin cannot move until mdstore lands on origin main, which needs a pull request.

Note on the working trees: ~/Projects/mdstore moved from refactor/split-review-check to gate/split-review-check mid-session, which broke one build that pointed at it. Builds now point at the ~/Projects/.wt/mdstore-per-tool worktree, which is stable.

Next, in order: land mdstore, then bump each pin to the merged commit, then commit each consumer, then write the three per-tool config files and delete ~/.config/mdstore/config.yml.
## Consolidated onto the existing branch (2026-08-17)

A branch already carried this work: per-tool-config, commit 31d50ed, dated 11:39 today, in the worktree ~/Projects/.wt/mdstore-wf. It was found only after a second branch had been written. The two agree on design, and their resolve.rs changes are byte-identical.

per-tool-config is the branch that continues. fix/per-tool-user-config (4082571) is abandoned; its unique pieces landed on top as e77ea6c: registry_path and Registry::load take the tool name, and the README user layer section is rewritten. The two issue commits were cherry-picked as ce0190d and 8e3d1e4.

One test difference kept, and earned by mutation: the pre-existing test asserts two tools differ and checks the tisket and zettel suffixes. It omits almanac. A mutation special-casing almanac back to the library directory passes every original assertion. The added loop over all three tools catches it, and the message names the tool.

mdstore has no missouri suite, so the merge gate runs cargo test only there. 202 tests pass with --all-features.

Consumer patches in the scratchpad target the same API and stay valid. Their pins still cannot move until this lands and 6wsp is resolved.
## Fresh-eyes round one: FIX FIRST (2026-08-17)

Three of five mutations were not caught. The reviewer was right on every count.

Not caught, and now fixed: registry_path discarding its tool argument, and both resolve.rs error strings reverting to the library path. The error strings are the whole user-visible point of this change, and nothing tested them. every_terminal_error_names_the_fix now asserts the tool's path is present and the library's is absent, on both error branches. registry.rs gained each_tool_owns_its_own_registry_path. Both were verified by re-applying the mutation.

Not caught, and not fixed: UserConfig::load and save_root each discarding the tool argument. A unit test cannot observe the derived path without writing under the real passwd home. Filed as sv4p.

The README had become false rather than merely silent: it claimed the registry follows the passwd-home rule while registry_path reads three repo-settable environment channels. The section now states the registry's real resolution. The rule itself is filed as zfip, and it is not a one-line fix, because every consumer's missouri fixtures set MDSTORE_REGISTRY.

registry_path used join(tool) where config_path used format!. join drops the base on an absolute component, so two public functions disagreed on what one tool value could reach. Both use format! now.

A comment claiming the suffix assertions miss a prefix mutation was wrong, and the mutation proved it: the ends_with assertion caught it first. The comment now states what the loop actually covers, which is a third tool the suffix checks omit.

Version is 0.4.0. Four public signatures changed, and repo precedent bumps for that.

203 tests pass with --all-features, clippy clean with --all-features, fmt clean. Round two is running against e1ccd7f.
## Fresh-eyes round two: FIX FIRST, and one blocker was my own bad fix (2026-08-17)

The round-one fix to registry_path was false. A leading {tool} in a format string is absolute for an absolute input, so join still dropped the base, and format!("/registry.yml") moved the empty-name case to the filesystem root, which the original code did not do. The comment asserted the containment property the code lacked. That is the round-one README defect repeated in a code comment, in the same change that was fixing it. Reverted to base.join(tool).join("registry.yml"), with a comment stating the escape plainly and pointing at sv4p.

The new registry test was vacuous under MDSTORE_REGISTRY. It returned early before asserting, so the suite reported 203 passed with the guarded bug live. Under that variable it now asserts the documented behavior: both tools resolve to exactly the forced path. It also missed prefix reintroduction, because ends_with matches trailing components and the env-dependent base gives nothing to anchor against. It now tests the whole string.

The sv4p claim that a unit test cannot observe the read and write paths was wrong, and the reviewer proved it by building the seam. config_path_in, load_in and save_root_in take the home as a parameter; public wrappers delegate through passwd_home. No public signature changed. The test plants a decoy under a temp home's .config/mdstore/ and the real file under .config/tisket/, then asserts the read takes the real file, the write returns a path in .config/zettel/, and the decoy keeps its bytes. Both mutations that round one missed now fail by name. sv4p keeps only its validation half.

Recorded because it repeated: two of three round-two blockers were prose asserting a property the code did not have. A comment is not a lower standard than a README.

204 tests, clippy and fmt clean with --all-features. Tip 2d64d65. Round three running.

## Release coordination (2026-08-17)

Another session is driving the six tools to release, and its decisions change the consumer half.

mdstore publishes to crates.io as package mdstore-core. 0.3.6 goes first as a baseline that claims the name and proves the pipeline, and this branch's 0.4.0 follows as an ordinary second release. The 0.4.0 bump stays.

Consumers already moved from a git rev to a crates.io dependency, so the planned step of bumping each pin to the merged mdstore commit no longer exists. Each consumer moves version 0.3 to 0.4 in the same change that adopts the tool argument. Until 0.4.0 publishes they build through a gitignored [patch.crates-io] mdstore-core path override, which stays in place. The three saved consumer patches are therefore stale in their manifest hunks and correct in their source hunks.

6wsp handed to that session, with the reproduction and the bisect range. It needs the answer before tagging, because the suspect commit is already inside 0.3.6 and the release workflow does not run missouri.

sv4p to be fixed on this branch before the pull request opens, because publication is what widens the caller set from three compile-time constants to anyone. It is not a traversal in any shipped binary today, and the issue should not claim it is. One open design point: rejecting a malformed name needs its own error, because None already means the home did not resolve, and reusing it makes the consumer's message lie.
## Rounds three and four, and one issue retracted (2026-08-17)

Round three found one blocker, and it was a weakened assertion. The seam's write test asserted a path suffix rather than equality against the home it was handed, so save_root_in could ignore that home and still pass. The mutation that exposes it is the refactor a careless hand writes, reaching for passwd_home() inside the inner function, and running that mutation writes into the real ~/.config before the assertion fires. A substituted temp home proves the same thing safely. Now equality.

Round four found the same failure mode a third time: a comment of mine asserting a containment the code did not have. Worse than the earlier two, because the change had also moved the code the less safe way. For tool = /etc, a per-component join gives /etc/config.yml and escapes, while the format string with its literal .config/ prefix gives ~/.config//etc/config.yml and contains. The format string is restored. ToolName is the first layer and the literal prefix is the second.

Three gaps closed in the same commit. is_plain_stem accepted a trailing separator under an off-by-one loop bound with the whole suite green, because every existing case put the separator in the interior; a/, a backslash and a NUL are now cases in both tests. ToolName::new rejects any colon, because a component carrying a Windows drive prefix replaces the accumulated path and this crate compiles for non-unix. The compile-time rejection is a compile_fail doctest rather than a claim in prose; a const item with a bad literal is a hard E0080 and fires even when nothing reads the const.

sv4p is resolved rather than deferred. The tool name is a validated type. Validation reuses store::is_plain_stem, which already encoded the rule and already argued the principle in its own doc comment: one predicate serves every tool. It was not const, so it is now, rewritten as a byte walk and proven equivalent over 1.4 million inputs by the reviewer. UTF-8 is self-synchronizing, so no multi-byte scalar carries a byte below 0x80, which is why a byte scan cannot get a false positive from a multi-byte sequence.

Vocabulary.tool is the same type now, so a consumer holds one const for the vocabulary, the config and the registry. That closes the half of the round-one finding that let a consumer print an error naming a file it never reads.

bzs1 RETRACTED and closed. The claim was that save_root reports a path inside the home while the bytes land outside. The physical file does sit in the link target, and the reported path still reads back: a load through it returns the written root_store. No false statement, nothing to fix, and a write that follows a symlink is what a symlink is for. The error survived three messages because the first test asserted only that the file exists in the link target, which confirms a symlink was followed and nothing more.

The tell that caught it: an inverted test, written to fail until the defect was fixed, passed on its first run. A test written to fail that does not is a statement about the premise, not about the code.

## Release coordination, current (2026-08-17)

6wsp is diagnosed by the release session. mdstore's sync_source resolves the declared pin after a fetch, so a pin deleted upstream fails the sync rather than reporting success and failing later on read. zettel's fixture expected exit 0 and now expects the refusal, which matches the contract the suite already sets in its ssh assertion. Closing here on per-tool-config, with the assertion name and the mutation verdict, when that session's reviewer returns.

A merge order binds six repos. gaff reaches main first, because scripts/merge-gate.sh in the release branches ends with gaff reviews check and the profile gaff lacks that subcommand. Then co.d takes a flake update and hms. Then the other five. Confirmed against the profile binary: bare reviews exists, check does not.

.gaff/gaff.yml conflicts between per-tool-config and the release branch. This branch has no reviews: key; theirs declares fresh-eyes and mutation. The bootstrap forces their branch first, so the rebase takes their version. Do not add the key here.

An absent reviews: key refuses every push, which is correct, but the message does not tell a reader what to write. Filed on the release side rather than fixed here.

Tip db5151a. 207 tests, 1 ignored for the network, 1 integration test, 2 doc-tests, clippy and fmt clean under the profile gaff. Round five running for a clean verdict, because a sign-off written on a FIX FIRST verdict is worth nothing.
