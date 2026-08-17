---
title: save_root reports a path inside the home while the write lands outside it
status: done
priority: null
assignee: null
due_date: null
labels: []
depends_on: []
created: 2026-08-17T20:48:57Z
updated: 2026-08-17T21:03:04Z
---

`save_root` reports a path inside the home while the bytes land outside it. Measured, not reasoned.

## What happens

A validated `ToolName` refuses a separator, a parent component and an absolute name, so the path `config_path_in` builds always sits under the home. The filesystem still redirects it. If `~/.config/<tool>` is a symlink to another directory, `create_dir_all` accepts it, the temp file and the rename both resolve through the link, and the bytes land in its target. `save_root` then returns `~/.config/<tool>/config.yml`, which is not where the file is.

Recorded by `userconfig::tests::a_symlinked_tool_directory_sends_the_write_outside_the_home`, which passes today and documents the behavior rather than approving it. The test also fails loudly if containment ever becomes stronger than this record.

## Severity, stated plainly

This is not a privilege boundary. Planting that symlink needs write access to the victim's `~/.config`, and anyone holding it can write the config file directly, or a shell rc, without any of this. Nothing escalates.

The defect is truthfulness. The returned path is wrong, so a caller that prints it tells the user a file is somewhere it is not. That matters here because the returned path is exactly what each consumer prints from `store root`.

It is not a publish blocker, and it should not be described as a path traversal.

## Options

1. Report the resolved path. Canonicalize the parent after the write and return that, so the printed path is where the bytes are.
2. Refuse a symlinked tool directory, which breaks a user who deliberately links their config directory into a dotfiles repo. That pattern is common, so refusing it is likely wrong.
3. Leave the behavior and document it on `save_root`.

Option 1 keeps the useful behavior and removes the false statement.

## Scratch Notes

## Not a defect. Closing (2026-08-17)

The premise was wrong, and the test that was meant to prove it passed instead.

The claim was that save_root returns a path inside the home while the bytes land outside it, so the reported path is false. The physical file does sit in the link target. The reported path is not false: a read of it resolves through the symlink and returns exactly what the write put there. Measured by a_symlinked_tool_directory_round_trips, which loads the config back through the reported path and gets the written root_store.

So there is no wrong statement to a user, and nothing to fix. A write that follows a symlink is what a symlink is for, and farming ~/.config is the reason a dotfile manager exists.

How the error survived three messages: the first test asserted that the file exists in the link target, which only confirms that a symlink was followed. It never checked whether the reported path still worked. An inverted test written to fail until the defect was fixed passed on the first run, which is the signal that caught it.

The test stays, as a normal test rather than an ignored one. It holds the property that matters: a symlinked tool directory round-trips.
