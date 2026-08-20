---
name: review-docs
description: >
  Judge every piece of prose a repository ships. Use as a required review,
  or as /review-docs. Not for writing docs: doc-editing covers that.
license: MIT
user-invocable: true
---

# Review: documentation

You are one independent check on a change you did not write. Judge the
prose. Read `signoff-driver` for the sign-off line, the two severity
bands, and the not-applicable case.

## Method

Three named standards carry this skill.

**Diátaxis** (Procida) splits documentation into tutorial, how-to,
reference, and explanation, and holds that mixing two in one document
serves neither.

**ASD-STE100** (Simplified Technical English) bounds a sentence. An
instruction holds at most 20 words. A description holds at most 25. Both
use the active voice and the simple present, with one term for one
concept.

**IBM's documentation quality characteristics** include accuracy: a
command in the documentation produces the output the documentation shows.

If `writing-docs-eval` is installed, load it for the full Diátaxis and IBM
catalogs; criteria 3 and 6 come from there. If `writing-sentence-level` is
installed, load it for the sentence tests. This skill adds the numeric
limits as a countable rule. If `humanizer` is installed, load it for the
catalog of AI-writing patterns.

## Criteria

1. **Sweep the repository, not the diff.** "This is unrelated to the
   change" is not a defense. Documentation rots by the change that did not
   touch it, so a diff-scoped review never catches it. Severity follows
   the criterion the finding lands under.

2. **Cover every prose surface.** The README, agent-instruction files,
   the contributing guide, bundled documentation, published skills, help
   text, error messages, and code comments. Error messages and comments
   are prose that ships. An agent-instruction file is the file a coding
   agent reads on entering a repository: AGENTS.md, CLAUDE.md, or the
   equivalent the repository names.

   A surface you did not sweep is **blocking** when it holds an
   instruction, and **minor** otherwise. Say which surfaces you swept.

   Commit titles, commit bodies, and pull-request text are prose too, and
   they are the surface most often written from inside the work. Judge
   them by criterion 8. A title reaches a reader who has none of the
   author's context: a list of commits, a release note, a blame view. "Fix
   the layers" names nothing there.

3. **Judge documentation types and their purity.** Does the repository
   carry a tutorial, a how-to, a reference, and an explanation? Does each
   document stay in one type? A missing type is **minor** unless a reader
   cannot start without it. Drift between types is **minor** for a
   paragraph and **blocking** when a reference no longer answers a lookup.

4. **Judge each sentence against ASD-STE100.** An instruction holds at
   most 20 words, and a description holds at most 25. Use the active voice
   and name the actor. Use the simple present. Use one term for one
   concept. Choose the simple word. Put the condition before the
   instruction. Use no idioms. A breach is **minor** on its own, and
   **blocking** where it makes an instruction ambiguous.

5. **Humanize, and hold the standard.** Remove AI-writing patterns.
   Removing them must not break ASD-STE100. **Minor.**

6. **Check the prose against the code.** Run the commands the
   documentation shows and confirm the output it claims. A documented
   thing that does not exist is **blocking**: a reader who follows it gets
   an error and no recovery. An undocumented flag is **minor**, and
   **blocking** only when a documented workflow depends on it.

7. **Check internal consistency.** Two documents may state different
   rules for one behavior. That is **blocking** when a reader must obey
   one of them, and **minor** for an example value. A reader cannot tell
   which document is current.

8. **Read it as a stranger, and hunt session residue.** Take every noun
   phrase that reads like an established term. Ask where a fresh reader
   learns it. A term the repository never defines is a finding, however
   natural it reads to the author.

   The common source is the work that produced the text. A correction, a
   debate, or a passing metaphor from the writing session hardens into
   vocabulary, and the author cannot see it because they were there. "The
   trifold gate matrix check" reads as a real thing to the person who
   coined it that afternoon, and as noise to everyone else. The tell is a
   definite article in front of a term the document never introduced:
   "the" says the reader already knows.

   Check invented compounds, capitalized phrases, and any word used in a
   private sense. **Blocking** when the reader must know the term to
   follow an instruction, **minor** otherwise.

   A definite article is the cheapest test. Read each "the X". Ask whether
   the repository names X, or whether the author named it this afternoon.
   Name it rather than pointing at it: write "config layers" where the
   work called them "the layers".

## Worked examples

### A rename leaves prose behind

A change renames a skill, and the diff applies the rename everywhere it
touches.

A weak review reads the diff and passes.

This skill greps the whole repository for the old name and finds it in a
script's error text, which the diff never touched. A reader following that
error looks for something that no longer exists.

    signoff[review-docs] FAIL 83bb8dd blocking: merge-gate.sh still
    names review-driver in its error text after the rename

### Session residue reads as vocabulary

Documentation says a list names "a review skill that the repository
vendors", and elsewhere refers to "the vendored library".

A weak review reads both sentences, follows them, and passes.

This skill greps for where "vendor" and "vendored library" are defined and
finds nothing. The term arrived from a conversation about distribution. A
reader who was not in it meets a definite article with no antecedent.

    signoff[review-docs] FAIL 0f00b85 blocking: "the vendored library"
    is required to follow the install instruction and is defined nowhere

### The sweep comes back clean

    signoff[review-docs] PASS 7c2d9ab swept 14 prose surfaces including
    error text and comments; ran every documented command and matched
    its output; no undefined term survives a grep; minor: two sentences
    in the tutorial run to 27 words
