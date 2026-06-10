# Identity and default-on idempotency

The point of ingest idempotency is to **avoid re-running the agent (which costs money) on a
source already ingested**. The identity model is a tiered fallback, and idempotency is the
default, not a mode.

## Identity tiers

1. **External id (authoritative).** When a source carries a stable business id — an email
   `Message-ID`, a YouTube `video_id`, a webpage URL — supplied via input frontmatter
   (`external_id:`) or `--external-id`, that id **is** the identity. If a success record
   exists for `(wiki_root, external_id, ingest_policy_sha256)`, the source is **skipped** —
   content drift is **ignored**. The id is a promise that "this is the same thing."
2. **Content hash (fallback).** With no external id, identity falls back to today's
   `(wiki_root, source_key, content_sha256, ingest_policy_sha256)`.
3. **Semantic similarity — explicit non-goal.** Fuzzy/threshold dedup needs a per-source
   embedding call (itself a cost), a stateful vector index, and a similarity knob — all of
   which contradict the deterministic ledger and the opinionated ethos. If near-duplicate
   notes ever become a real problem, that earns a separate *advisory* `dedup` command that
   reports candidates for a human — never a silent skip inside ingest.

`ingest_policy_sha256` (hash of `ingest.md`) is in **both** keys, so changing the ingest
policy deliberately re-ingests everything, even seen ids.

## Default-on idempotency

The ledger (`.agwiki/ingest-state.jsonl`) is **always written and always consulted**.
The previous opt-in `--resume` flag is **dropped** — idempotency is how the tool behaves, not
a mode to remember. The single override is **`--force`**, which re-ingests even a seen id.

## Considered and rejected

- **Conjunctive id + content** (re-ingest a same-id source whose bytes changed). Rejected:
  it re-pays for every trivial byte drift, defeating the cost-avoidance goal. Mutable sources
  are the minority; `--force` recovers the rare intentional re-ingest.
- **Per-child-directory batch (#42).** Dissolved: with one wiki-level ledger and id-based
  skipping, re-ingesting a whole tree is already cheap and safe, so per-channel iteration
  buys nothing. Per-channel *reporting* falls out of grouping the `IngestEvent` stream by a
  source's parent directory.

## Consequences

- Ledger schema gains `external_id: Option<String>`; `schema_version` bumps to 2 (missing
  field reads as `None`).
- A first-time user who wants to re-run is told by the skip notice to use `--force`.
- `IngestEvent` unifies skip reporting (#40) and dry-run/plan mode (#41, all `Planned`
  events, no agent run).
