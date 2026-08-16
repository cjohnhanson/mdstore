---
title: 'the prime contract: what a tool says about itself, and what it never says'
status: todo
priority: '2'
assignee: null
due_date: null
labels:
- prime
- contract
- design
depends_on: []
created: 2026-08-16T19:08:20Z
updated: 2026-08-16T19:08:20Z
---

The contract for `<tool> prime` across tisket, zettel, gaff, and almanac. Three independent designs, six adversarial attacks, one synthesis, one cold-reader check against the ground facts. The check's findings are folded in below.

## Why

A tool's prime describes the tool. It never decides policy. "Use almanac instead of skills" or "read the active issue at session start" is a decision, and it belongs to whoever configures the session: a gaff hook, a reminder, or a CLAUDE.md for someone with no gaff. A prime that says it is overfit to one user's setup and wrong for the standalone user. tisket's current prime says "this repository uses tisket" and prints a workflow; both are policy, and the location claim is false for a user-level tracker declared through stores.yml.

## Rules

1. **Prime describes; it never directs.** No "always", "never", "at session start", "before you", no numbered workflow. The same bytes reach every host and user.
2. **The only "must" is a fact the binary enforces.** Enforced in code, not stated in docs. zettel `note review` is a documented rule, so the prime states its mechanics only. `gaff allow` no longer refuses a non-tty (that check was removed on 2026-08-15; the built-in guard in the hook is what refuses it from an agent), so gaff's prime must not claim a terminal-only rule for it.
3. **Prime is a pure function of the binary.** No arguments, config, cwd, env, store, network, or stderr. Exit 0. This makes build-time capture exact and testing by string equality possible. `tisket prime` moves off `Repo` to a free function.
4. **Dynamic facts are separate commands the host chooses to inject.** `store list`, `status`, `issue list`, `almanac index --md`. Every runtime attack landed on the derived line: wrong vantage direction, per-document git spawn, staleness, dropped first under budget, an `init` hint that invites `tisket init` in foreign repos.
5. **Prime names no sibling tool, no host, no harness syntax, no skill.** No gaff, mdstore, skills.sh, Claude Code, `!`, `/zettel`. This applies to `--help` strings too, and to almanac's README, which today says "wire it into gaff as a prime section" — advice that names a sibling and is wrong on its own terms (a gaff section is file-only).
6. **Store-aware, location-agnostic.** A prime says how a store is found (`--root <dir>`, `stores.yml`, `<tool> store list`) and never where one is. A default may be stated only as a default. A user-level store declares project stores, never the reverse, so any location claim is wrong for someone.
7. **Fixed shape.** Line 1 `# <tool>`. Line 2 the `--help` about string, from the same const, byte-equal (almanac's about has no trailing period; the prime must match it or the const changes). Two to four sentences of model and enforced facts. `Commands:` with at most seven lines, each a full invocable form starting with `<tool>`, no gloss column. Last line `More: <tool> --help; <tool> docs`. No headings below line 1, no tabs, no control chars, no `[gaff:` substring, one trailing newline.
8. **Command lines are hand-written and machine-checked.** `a|b` alternation only where every alternative takes the same remaining arguments, and the test expands it. So `tisket scratch <id> read|append|write` is not allowed: `append` takes text and `read` does not. Selection criterion, stated once: the commands that read the store plus the writes that produce the tool's primary artifact. Setup, sync, curation, serve, and terminal-only commands are excluded.
9. **`More:` names `--help` and `docs` only.** No topic list, no `docs search`.
10. **≤ 700 bytes UTF-8, tested.** No `--max-bytes` ladder.
11. **Each tool ships one test:** bytes; shape; every `Commands:` line resolves against the tool's own command table (clap walk for tisket/zettel/almanac; gaff's dispatch table, since gaff is hand-parsed); every flag on the resolved subcommand exists. Where prose names a flag (`--approve` takes a value), the test covers it too.
12. **No policy slot inside the binary.** tisket's `additional_instructions` is no longer read; the key stays parseable, `tisket init` stops writing it, `tisket check` reports it as moved.
13. **The contract has one home: this repo**, since mdstore's README already lists all four tools. Each tool's cli-reference carries two sentences: "Prime depends only on the binary version. Put it into an agent's context; policy about when to use the tool belongs to the caller."
14. **Shape is stable across releases; wording is not.**

## What each tool builds

Only tisket has `prime` today. zettel, gaff, and almanac build it new. tisket's changes shape (rules 1, 3, 6, 8, 12). Each is a tisket issue in its own repo, depending on this one.

## Per-tool outlines

Drafts, measured. Each under 700 bytes.

**tisket** — about string; an issue is a markdown file with frontmatter and a fixed status; a tracker is a directory with tisket.yml, `--root` names one, default is cwd; stores.yml may declare other trackers under aliases and an id can read `alias:id`; a declared tracker is read-only (enforced); body and scratch are separate. Commands: `issue list [-s] [-p]`, `issue show <id>`, `issue create <title> [-p] [--body-file <f>]`, `issue close <id>`, `scratch <id> read`, `scratch <id> append <text>`, `search <pattern>`. Omits: workflow steps, "this repository uses", what the body holds (policy), status vocabulary, `additional_instructions`.

**zettel** — about string; notes link by `[[id]]`; a store is a directory `zettel init` created, `--root` names one, stores.yml may declare others (`[[alias:id]]`); every span has a provenance: `human[:name]`, `agent[:summary|index|inference]`, `citation[:source]`; missing is unknown, unknown is never promoted to human (enforced); `note review <id> --approve <all|N,N>` writes a reviewed stamp; `read --provenance` filters spans. Commands: `search`, `read [--tag] [--provenance]`, `context <id>`, `note create <title> -t <tags> -p <origin[:qualifier]> -b <body>`, `store list`. Omits: "only a human runs review" (not enforced), `.zettel/`, "next to the code", "search before researching", marker syntax.

**almanac** — about string (no trailing period); a skill is a directory with SKILL.md; a library is a directory with almanac.yml, `--root` names one; each entry pinned to commit and hash; stores.yml may declare other libraries, nearer wins a name collision. Commands: `list`, `show <name>`, `index [--md] [--max-bytes <n>]`, `status`, `store list`. Omits: the skill list itself, replace-vs-supplement, skills.sh, add/update/remove/sync.

**gaff** — about string; from the host's hooks it runs guards on tool calls, injects sections and reminders at session start and on a cadence, and can hold the stop until a reminder clears; each injected entry opens with a tag, `gaff:<name>` in square brackets, on its own line; a refused tool call names its guard; repo config `.gaff/gaff.yml`, user config `~/.config/gaff/`. Commands: `doctor`, `status`, `remind <text> (--after <n> | --at stop) [--id <id>]`, `remind --clear --id <id>`, `check`. Omits: `!` (Claude Code syntax), the goal-hold habit (policy), profiles, trust, handlers, this repo's live guards (that is `doctor`), and any terminal-only claim for `allow` (see rule 2).

## Assembly, for a gaff user

Build-time capture into user sections. home-manager runs `<tool> prime > ~/.config/gaff/prime-<tool>.md` from the same derivation as the binary on PATH, so drift is zero by construction; a prime that exits non-zero fails `hms` loudly. gaff.yml user sections: `voice` (refresh 80), the four primes (no refresh: compaction re-primes at SessionStart), then `ecosystem` (refresh 120), which becomes **policy only** — the rule between the tools, the session-start ritual, the stop-hold habit, "search before researching", "close never a status edit" — with no command flags in it, because the prime above it in the same flush carries the flags. Budget: voice 1870 + primes ~2660 + policy ≤ 800 + headers, under 6144; raise the cap to 8192 so the repo layer keeps room. A co.d check asserts the user layer's SessionStart total. `ecosystem.md` as it stands today, description and policy in one file, is retired: description twice, one copy drifting.

For a CLAUDE.md-only user: policy in CLAUDE.md; description via a SessionStart command hook whose stdout is `<tool> prime`. For a codex user: policy in AGENTS.md; description as a fenced block between markers, regenerated after an upgrade. Where `serve` is used, the same string is the MCP `instructions` field.

CLAUDE.md edits: Working Notes shrinks to the two-sentence rule plus a pointer to the gaff section. The "use skills.sh rather than almanac" clause is in **Project Skills**, not Working Notes; it folds into ecosystem policy as one sentence.

## Rejected

Runtime-derived lines in the prime; primes as gaff handlers (exact-cwd trust, 500 ms deadline, tail-cut removes `More:` first); `--max-bytes` ladders; a native `[gaff:prime]` entry; any policy slot inside a binary; enumerated docs topics; `|`-collapsed subcommands with different arguments; literal `[gaff:<name>]` or `!gaff allow` in gaff's prime; "only a human runs review" as prime text; location claims; a combined meta-prime; `command:` on repo sections; JSON prime rendered by gaff; cadence refresh on primes; keeping ecosystem.md as description plus policy.

## Not verified

Whether a Claude Code SessionStart hook's stdout is injected on compaction as well as start; whether codex has a command hook.
