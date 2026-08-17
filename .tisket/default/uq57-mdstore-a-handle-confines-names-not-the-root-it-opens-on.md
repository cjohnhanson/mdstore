---
title: 'mdstore: a handle confines names, not the root it opens on'
status: todo
priority: null
assignee: null
due_date: null
labels:
- review-followup
depends_on: []
created: 2026-08-17T02:25:13Z
updated: 2026-08-17T02:25:13Z
---

A handle confines the names used under a root. It does not choose the
root: `StoreDir::open` resolves that with `Dir::open_ambient_dir`,
which uses the authority this process already holds.

Where the root is named by content the reader does not control, the
content chooses what the handle confines to. Two instances are proved,
in two repositories, by two independent reviewers:

1. mdstore, `LocalPaths::locate`. A vendored dependency declaring
   `path: ../../secret` gets a `StoreDir` rooted at the secret.
   `anchored_to_one_machine` refuses an absolute path and a `~` anchor,
   and permits a climbing relative path, because `../project` is the
   documented sibling pattern.

2. almanac, `show_reference`. A library shipping `references -> /etc`
   served `/etc/hosts` through `almanac show`. A relative link does the
   same, and a tarball and a git checkout both carry that form. Fixed
   in almanac by refusing a linked directory by type before the handle
   opens, so a name never reaches a root that was never opened.

The almanac fix is local and works. It does not generalise: every
consumer that opens a handle on a path some content named has to
remember the same check, which is the burden the capability exists to
remove.

Options, none decided:

- Document the boundary on `StoreDir::open` in the terms above, so a
  consumer knows the root is its own problem. Cheapest, and honest.
- Offer a constructor that opens a subdirectory through an existing
  handle, so a root derived from content inherits confinement rather
  than resolving ambiently. Covers the almanac shape directly.
- Settle the trust parameter for declared store paths. The other
  session owns that work and it is the only thing that closes case 1,
  because a path check cannot: climbing one level is the documented
  pattern.

The module header currently says what this does not cover, and this is
not in the list.
