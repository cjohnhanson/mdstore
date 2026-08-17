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
