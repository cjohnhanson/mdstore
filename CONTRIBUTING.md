# Contributing

## Use of AI

Say so in the pull request when a coding assistant wrote part of the
change. Disclosure is expected, not disqualifying.

You must be able to explain the change in your own words. Write your own
comments on the pull request. A pull request the author cannot explain
gets closed.

## Setup

```sh
git clone https://github.com/cjohnhanson/mdstore
cd mdstore
cargo build
cargo test --workspace --all-features
```

Pass `--all-features`. The `mcp` feature is off by default, and a test
run without it never compiles that code.

## Open an issue first

Open an issue before a large change. A small fix needs no issue.

Issues live in the repository as markdown files under `.tisket/`. Read
them with `cat`, or with [tisket](https://github.com/cjohnhanson/tisket):

```sh
tisket issue list
tisket issue show <id>
```

## The gates

Two gates decide whether a change lands. CI runs both on every pull
request, as the check named `gate`. `main` is protected and requires
that check, so a red gate blocks the merge.

The commit gate runs three things:

```sh
# 1. It refuses a commit when unstaged .rs changes differ from the index.
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The merge gate runs the tests, then requires a review note:

```sh
cargo test --workspace --all-features
```

A push carries a review note on its tip. A reviewer who did not write
the change reads it, then removes a guard the change adds and watches a
named test go red. The note records both:

```sh
git notes --ref=reviews add -m \
  'fresh-eyes: <reviewer> <scope>. Mutation: <what> -> <test> red' <sha>
```

A note that only says the change was read is refused. That rule exists
because every regression that reached a reviewed tip on 2026-08-16 had
green tests and a review note, and each was caught only when a reviewer
put the bug back.

## Running the gates locally

The gates are declared once, in `.gaff/gaff.yml`. CI reads that file, so
CI and a local run cannot drift.

**Do not install the hooks for a one-off contribution.** They refuse a
push without a review note, so an outside contributor cannot push at
all. Open a pull request and let CI run the gates.

For sustained work, install them with
[gaff](https://github.com/cjohnhanson/gaff):

```sh
cargo install --git https://github.com/cjohnhanson/gaff
gaff init --git
gaff trust
```

Run `gaff trust` from your own shell. The hook refuses it inside an
agent session, so an agent cannot grant itself command execution.

To run the gates without committing:

```sh
gaff ci
```

## Pull requests

1. Branch from `main`, and open the pull request from a fork.
2. Keep the change and its tests together.
3. Add an entry to `CHANGELOG.md` for a user-visible change.
4. Write the commit message in the imperative present. State what the
   change does. State why where the diff does not show it.

## What not to commit

`.gitignore` covers these. No gate checks them, so read the list:

- Anything under `target/`.
- An absolute path naming a home directory. It exposes an account name,
  and it breaks every other clone.
- A `path = "..."` dependency override pointing outside the repository.
  Put it in `.cargo/config.toml`, which is ignored.
- Local editor or coding-agent state.

## Questions

Open an issue on GitHub for a question about the library.

## Security

Do not open a public issue for a vulnerability. See
[SECURITY.md](SECURITY.md).

## License

Your contributions are licensed under the MIT license, the same as the
project.
