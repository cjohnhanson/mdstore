---
title: the tool name is unvalidated, and the read and write paths are untested
status: todo
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-17T20:21:27Z
updated: 2026-08-17T20:21:27Z
---

Five public functions take a `tool: &str` that nothing validates, and the two that open or write the file have no test.

## Unvalidated name

`config_path` builds `~/.config/{tool}/config.yml` with `format!`. An empty name yields `~/.config/config.yml`, so every caller passing it collides on one file. `".."` yields `~/config.yml`. `"../../../../tmp/evil"` leaves the home directory, and `save_root` calls `create_dir_all` on the parent and writes there.

Not exploitable today: inside the crate every caller passes a test literal, and each consumer passes a compile-time constant. It stays worth a guard, because these functions are `pub` in a crate that carries publish metadata, and the module doc holds this file to gaff's standard for security config. Rejecting anything that is not one plain path component is one line.

## Untested read and write

`UserConfig::load` and `UserConfig::save_root` were each mutated to discard the tool argument and call `config_path("mdstore")`. The suite stayed green. `Registry::load` has the same gap.

A unit test cannot observe the derived path without writing under the real passwd home, which no test may do. Two candidate seams: return the resolved path from `load` the way `save_root` already does, or exercise the fixed path through a consumer's missouri suite with a fake home. Every existing missouri fixture passes `--user-config`, so the fixed path is exercised nowhere in any repo.

## Also open

Nothing ties `Vocabulary.tool` to the `tool` passed to `config_path`. A consumer that spells them differently prints an error naming a file it never reads. One type used by both would make that a compile error.

Found by the fresh-eyes review of ccgh.
