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
