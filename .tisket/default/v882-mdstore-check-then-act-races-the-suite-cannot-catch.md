---
title: 'mdstore: check-then-act races the suite cannot catch'
status: todo
priority: null
assignee: null
due_date: null
labels:
- review-followup
depends_on: []
created: 2026-08-17T02:42:28Z
updated: 2026-08-17T02:42:28Z
---

Four half-guards survive every test in the suite. Each is one shape: a
check that is correct at the instant it runs, followed by an action
that resolves the path a second time and independently. Each is a
race, and no deterministic unit test separates them from correct code,
because catching one needs a path swapped between the two calls.

1. Confirm the path through the handle, then act with ambient std::fs.
2. Canonicalize the joined path, check starts_with(root), then act
   ambiently.
3. Walk the components by hand looking for a link, then act ambiently.
4. Re-open the store root through Dir::open_ambient_dir on every call,
   with every operation still going through a cap-std Dir.

The fourth is the one worth acting on. It is the exact regression the
module header records as already fixed once: re-opening the root per
operation resolves it through ambient authority every time, so a root
swapped between two calls is followed. That lesson lives in prose and
is enforced by no test. It is also the likeliest of the four to come
back, because it looks completely confined.

a_swapped_root_does_not_redirect_an_open_store pins the property for
read and write by moving the store and planting a link. It does not
pin it for a handle that re-derives itself, because the mutation keeps
using a Dir.

Related, from the almanac review of the same class: the check-then-open
race in a consumer was measured. Forty thousand calls against a flipper
did not win it; widening the window to 200 microseconds between the
check and the open took it 45 times in 2000 attempts. So the window is
two syscalls wide and real, and winning it needs a live adversarial
process with write access to the directory being read.

Depends on the store-root question in the sibling issue: a constructor
that opens a subdirectory through an existing handle would remove the
reason a consumer re-resolves a root at all.
