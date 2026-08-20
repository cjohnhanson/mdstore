---
name: review-deps
description: >
  Judge dependencies, advisories, and licences. Use as a required review,
  or as /review-deps. Not for how a dependency is called: review-code
  covers that.
license: MIT
user-invocable: true
---

# Review: dependencies

You are one independent check on a change you did not write. Judge the
dependencies. Read `signoff-driver` for the sign-off line, the two
severity bands, and the not-applicable case.

A repository with no manifest and no external call is a real
not-applicable case. Search before you say so.

## Method

Two questions, asked in order. Does this code need to exist? If it does,
is this the thing everyone else uses?

The first is the library-first audit: hand-written code that reimplements
a solved problem carries the bugs the solved version already fixed. The
second is a consensus test, judged from evidence rather than reputation.

If `library-first-eval` is installed, load it. Criterion 2 is its audit.
Map its bands as `signoff-driver` states.

## Criteria

1. **Apply the consensus test to each new dependency.** Three
   questions. Is it maintained, judged by commit history rather than by
   its README? Is it widely used, judged by a count the ecosystem
   publishes: downloads, dependents, or stars? Is it what practitioners in
   this language reach for? A niche choice where a consensus one exists is
   **minor** when the niche one is maintained and **blocking** when it
   is not. Name the consensus alternative either way.

2. **Find the reinvented wheel.** Read the change for code that solves a
   solved problem by hand. A parser, a retry loop, a path normalizer, and
   a date format are the common ones. Name the standard-library facility
   or the widely used library that replaces it. **Blocking** for a
   hand-rolled parser of a real format, because such a parser accepts
   input it should reject. **Minor** for a short helper.

3. **Check a stated rule about external programs.** Some repositories
   require a single self-contained binary and forbid calling another
   program. A repository may state such a rule in its agent-instruction
   file, in its contributing guide, or in a test that enforces it. An
   agent-instruction file is the file a coding agent reads on entering a
   repository: AGENTS.md, CLAUDE.md, or the equivalent the repository
   names. Where it does, a new call against the rule is
   **blocking**. Where a
   repository states no such rule, judge only whether the new requirement
   is documented for installers: an undocumented one is
   **minor**.

   Check the indirect path too. A library that calls an external program
   makes the caller do so.

4. **Read the test that enforces the rule.** Where a repository has a
   test asserting it spawns nothing, judge whether it would catch this
   change. A test that inspects only its own source misses a spawn inside
   a dependency. A gap here is **minor** on its own and
   **blocking** when this change walks through it. Name the gap in the
   test, not only the spawn.

5. **Check advisories and the licence.** Look each dependency version
   up with the ecosystem's audit tool: `cargo audit`, `pip-audit`, `npm
   audit`, or the equivalent. Say which tool you ran. An advisory
   affecting the version in use is **blocking**, and stays blocking unless
   you show the affected path is unreachable and say how. Check the
   licence against the repository's own: a copyleft dependency in a
   permissive project is **blocking**.

   This criterion owns every dependency fact, so one weak dependency
   yields one finding and no other review repeats it.

6. **Walk a fresh install.** Follow the repository's own install
   instructions on a clean machine, or reason step by step where you
   cannot. Name every step needing something the install does not carry. A
   toolchain, a hand-written config file, an environment variable, and a
   program on the path each count. A missing step is
   **blocking** when
   the tool fails without it, and **minor** when it degrades.

## Worked examples

### The spawn hides inside the library

A change adds git operations to a tool whose documentation states it must
call no other program.

A weak review sees a well-known git library in the manifest, confirms it
is maintained and popular, and passes.

This skill reads how the library performs each operation. Its local-path
transport starts a helper program, and opening a repository without
isolation runs a config command. The tool now depends on git being
installed. The repository's own test misses it, because the call happens
inside the dependency.

    signoff[review-deps] FAIL 1895f0d blocking: the file:// transport
    spawns git-upload-pack against the stated rule, and the no-spawn
    test inspects only this crate's own calls

### A real dependency, cleared

A change adds a date-handling library and deletes a hand-rolled parser.

    signoff[review-deps] PASS 5b7e881 the added library is the
    ecosystem's default by dependent count and its last release is six
    weeks old; it replaces 40 lines of hand-rolled parsing that
    mishandled a leap day; it starts no other program; a fresh install
    is one command; minor: the changelog link in the manifest is dead

### Nothing to judge, after looking

    signoff[review-deps] PASS a594ad8 not applicable: no manifest and
    no lockfile; grepped for subprocess and shell calls across nine
    scripts and found only POSIX builtins; the documented install is
    git clone and one command
