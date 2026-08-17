# Security policy

## Reporting

Do not open a public issue for a vulnerability.

Report it privately:
**https://github.com/cjohnhanson/mdstore/security/advisories/new**

That opens a thread only you and the maintainer can read.

Include what an attacker gains, what they must already control to get
it, the affected commit, and steps that reproduce it.

## What happens next

mdstore has one maintainer, so response is best effort. Expect a reply
within a week.

A confirmed report gets a fix and an advisory published together. You
are credited unless you ask otherwise.

## Scope

mdstore reads and writes markdown documents under a directory. It
resolves a store graph from `stores.yml` declarations, and it fetches
from three source types: a local path, a git repository, and an https
blob prefix.

A store declaration can come from content the reader does not control,
such as a vendored dependency or a remote store. What that declaration
can reach is the boundary worth attacking.

In scope:

- A document or a store declaration reaching outside the directory it
  should be confined to.
- A fetch reaching a host or a path that the declaration did not name.
- Reading untrusted content leading to code execution.
- `~/.config/mdstore/config.yml` naming a root where a write lands. That
  file is security config. It resolves the home directory from the
  passwd database, never from an environment variable, because every
  environment channel is settable by a repository.

Out of scope:

- A dependency advisory with no exploitable path through this library.
  Report it to that dependency.
- Denial of service from a malformed local file, where the caller
  already controls that file.

## Known boundaries

Documented limits are not vulnerabilities. `src/confined.rs` carries a
`# What this does not cover` section in its module documentation. Read
it before reporting a traversal issue.
