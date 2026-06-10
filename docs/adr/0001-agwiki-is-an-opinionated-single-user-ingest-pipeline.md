# agwiki is an opinionated single-user ingest pipeline

agwiki is built for a single person or a small trusted group (a family, a small team).
There is **no security model, no auth, and no multi-tenancy** — all input is trusted
because it is the operator's own. Because of that, the tool is free to be **strongly
opinionated**: where a real alternative exists we pick one way and do not make it
configurable.

We also decided agwiki **is the pipeline**, not merely a single-source primitive: it owns
the stages from a local artifact onward (ingest → content model → materialize), the
identity/idempotency ledger, and the post-step hooks. It does **not** fetch from remote
services — its left edge is a raw artifact already on local disk — which keeps the
"no auth" principle intact (no OAuth tokens, no API credentials).

## Consequences

- This **reverses the README's prior scope line** ("agwiki does not handle PDF or YouTube").
  Those formats are now in scope *as inputs you have already pulled down and normalized
  to markdown* — the fetching/normalizing remains external glue, but agwiki is no longer
  positioned as a narrow single-file tool.
- Features are judged against "opinionated, no flexibility knobs": plugin systems, config
  interpreters, and runner abstractions are rejected by default (see ADR 0002, 0003).
- The README must be updated to describe the pipeline scope and the no-fetch boundary.
