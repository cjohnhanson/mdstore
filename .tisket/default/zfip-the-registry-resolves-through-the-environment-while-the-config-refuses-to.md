---
title: the registry resolves through the environment while the config refuses to
status: todo
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-17T20:21:27Z
updated: 2026-08-17T20:21:27Z
---

Two config files in one crate resolve by opposite rules. `userconfig.rs` states the doctrine for itself: no `XDG_CONFIG_HOME`, no `MDSTORE_CONFIG`, no `$HOME`, and the home directory comes from the passwd database, because every environment channel is repo-settable and the file names where a write can land. `registry_path` reads `MDSTORE_REGISTRY` first, then `XDG_CONFIG_HOME`, then `$HOME`.

The registry is the larger surface. `root_store` names where a write lands. The registry decides which content a declared git URL resolves to, so a repo that sets `MDSTORE_REGISTRY` in `.envrc` chooses what every command reads, and nothing in the output says so.

Found by the fresh-eyes review of ccgh, which rated it the most severe finding. The README now states the weaker rule plainly instead of claiming the stronger one, so nothing is currently false. The rule itself is unresolved.

## Why it is not a one-line fix

Every consumer's missouri suite sets `MDSTORE_REGISTRY` to a relative path, and the fixtures depend on it. Dropping the channel turns those suites red until each one adopts another seam. The user config solved the same problem with a hidden `--user-config` flag, on the stated grounds that a flag is visible in a transcript where an environment variable is not. That seam is the obvious model.

## Decide

1. Registry adopts the passwd-home rule, plus a hidden `--registry` flag for tests. Three consumer suites change with it.
2. Registry keeps the environment channels, and both module docs state one rule and its exception, so the crate stops contradicting itself.
