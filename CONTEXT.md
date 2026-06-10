# agwiki

An agent-driven markdown wiki tool for a single person or a small trusted group (a family, a small team). It ingests sources into a wiki, validates links, and exports skill bundles. No security model and no multi-tenancy — it is strongly opinionated by design.

## Language

**Ingest**:
The agent-driven step that reads a source's content and writes it into the wiki's content model. Agnostic to input format — it consumes text/markdown and does not parse or fetch source formats (turning an email, webpage, or video into markdown is upstream glue, not agwiki).

**Materialize**:
The deterministic step that renders the content model into a concrete consumable layout for a given target (`wiki` for humans, `skill` for agents, `html`). Subsumes what were separate compile/export commands; the wiki is just one target, not privileged.
_Avoid_: compile, export, render. _Not_: pre-ingest staging — materialize is strictly the output side.

**Content model**:
The structured entities a wiki holds (kinds such as concepts, topics, projects, people, syntheses, sources), living under `content/`. The pivot both ingest and materialize are agnostic about: ingest fills it, materialize gives it form.
_Avoid_: ontology (reserve for the kind/relation schema only)

**Target**:
A layout materialize produces from the content model — `wiki`, `skill`, or `html`.

**Source**:
A single thing being ingested — one email, one video transcript, one note. The unit of ingest and of identity.
_Avoid_: item, document, file

**External id**:
A source-provided stable business id used to recognize a source across re-runs — an email `Message-ID`, a YouTube `video_id`, a webpage URL. The primary identity tier; preferred when available because it skips re-ingesting (and re-paying for) a source already seen.
_Avoid_: unique_id, uuid, source_id

**Ledger**:
The append-only `.agwiki/ingest-state.jsonl` record of successful ingests, consulted to skip sources already ingested under the same identity.
_Avoid_: manifest, journal, cache
