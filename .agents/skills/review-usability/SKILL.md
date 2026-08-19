---
name: review-usability
description: >
  Judge whether a newcomer and a program can use a tool. Use as a required
  review, or as /review-usability. Not for visual design.
license: MIT
user-invocable: true
---

# Review: usability

You are one independent check on a change you did not write. Judge whether
the tool can be used. Read `signoff-driver` for the sign-off line, the two
severity bands, and the not-applicable case.

## Method

Run the tool before reading its source. Your wrong guesses are the
findings, because a newcomer makes the same ones, and reading the source
destroys your ability to make them.

Then three named lenses. **Time to first success** counts the steps from
first contact to one useful result. **Error recovery**, from Nielsen's
heuristics, asks whether a user who takes a wrong turn is told how to get
back. **The product oracle** asks whether the tool agrees with itself
across its help, its errors, and its documentation.

If `qa-cli` is installed, load it. Its phases cover criteria 1, 3, 4, and
6. Map its bands as `signoff-driver` states. If `agent-interaction-design`
is installed, load it for criterion 3. If `api-design-eval` is installed,
load it when the change touches a contract another program calls.

## Criteria

1. **Run it cold.** Start from `--help` alone. Record every point where
   you guessed and were wrong. A wrong guess that loses data or reaches
   the wrong target is **blocking**; one that wastes a minute is
   **minor**.

2. **Measure time to first success.** Count the steps from first
   contact to one useful result. Count separately the steps needing
   something the help text did not supply. More than three of that second
   kind is **blocking**, and one to three is **minor**. A tool nobody can
   start is not usable.

   Then judge what the tool says when it acts. Silence on success is a
   convention many tools follow, and it is right where the effect is
   visible. Silence is **blocking** where a user cannot confirm what
   happened without a second command. A write to a path the user did not
   name, a partial success, a no-op resembling success, and any
   destructive act are each of that kind. So is a run long enough to
   resemble a hang.

3. **Grade the surface a program sees.** Output a caller must guess
   at, rather than parse, is **blocking**. Is it parseable without a
   heuristic? Does every refusal name its cause and its fix? Is the exit
   code meaningful and distinct? A program cannot ask a follow-up
   question. A refusal stating only the fault is **blocking** where the
   message names no flag, file, or value to change. It is
   **minor** where it names one.
   Two states that a caller cannot tell apart from output and exit code
   together are **blocking**.

4. **Hold one term to one concept.** The help, the errors, the
   documentation, and the flag names must use one word for one thing.
   **Blocking** when two words for one concept appear in one workflow,
   **minor** when they sit in different documents.

5. **Judge documentation as part of the tool.** The documentation may
   explain a step the tool could make unnecessary. Say which change would
   delete that paragraph. **Minor**, unless the step is required for
   correct use and easy to miss.

6. **Take a wrong turn on purpose.** Run what a confused user runs.
   Try a missing argument, a wrong order, an absent flag, and the command
   in the wrong directory. Judge each refusal by criterion 3.

## Worked examples

### Two states one caller cannot separate

A change adds a command printing a configured list.

A weak review runs it where the list is configured, sees the right output,
and passes.

This skill takes the wrong turns: it runs the command where no config
exists and where the config declares an empty list. Both print nothing and
exit zero. A caller cannot tell "no policy here" from "policy exists and
is empty", so a gate reading that output requires nothing in both cases.

    signoff[review-usability] FAIL 0f00b85 blocking: an absent config
    and an empty list are indistinguishable on stdout and exit code

### The same command, after the split

    signoff[review-usability] PASS 62628cd ran it cold from --help in
    six states including no config and a truncated one; each exit code
    is distinct and each refusal names its fix; first success took one
    step; minor: the error says "repo" where the docs say "repository"
