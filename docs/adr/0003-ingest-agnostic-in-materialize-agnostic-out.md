# Ingest agnostic-in, materialize agnostic-out; no source-format adapters

agwiki's pipeline has two verbs around a content model, and agwiki is agnostic at **both**
edges:

```
raw content  →  ingest (agent)  →  content model  →  materialize (deterministic)  →  wiki | skill | html
```

- **ingest** consumes text/markdown and is **agnostic to input format**. It does **not**
  parse or fetch source formats — turning an email, webpage, or video transcript into
  markdown is upstream glue. There are **no built-in source-type adapters** and no plugin
  system for them; adding a "source type" is explicitly not a thing agwiki does.
- **materialize** renders the content model into a concrete **target** layout (`wiki` for
  humans, `skill` for agents, `html`) and is agnostic to who consumes it. It **subsumes**
  the former separate `compile` / `export skill` / `export html` commands into one verb
  (`materialize --target <t>`); the wiki is just one target, no longer privileged.

## Considered and rejected

- **Source-type adapters in core** (parse `.eml` for `Message-ID`, etc.). Rejected: it makes
  core format-aware and fragile against format drift, and contradicts "opinionated, small."
  The external id instead rides *with* the content as input frontmatter (`external_id:`) or
  a `--external-id` flag.
- **Issue #39 as originally written** ("materialize" = a *pre-ingest* staging/normalization
  step). Rejected and the word repurposed for the output side. Pre-ingest normalization is
  external glue.

## Consequences

- Adding a new output target is a code change to the materialize stage (acceptable — no
  plugin system, by design).
- `check sources` and `check wiki` remain validation verbs distinct from `materialize`
  (checking is not producing).
