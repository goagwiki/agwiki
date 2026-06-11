# Agent Wiki

**agwiki** is an agent-driven wiki pipeline for a single person or a small trusted group (a family, a small team). It **ingests** sources into a structured content model, then **materializes** that model into the layout you want — a browsable wiki, an [Agent Skill](https://agentskills.io/) bundle, or static HTML.

The shape is two verbs around a content model, agnostic at both edges:

```
 raw content            ┌─────────┐    content model       ┌─────────────┐     targets
 (normalized       →    │ ingest  │ →  (entities under  →   │ materialize │  →  • wiki   (humans)
  markdown)             │ (agent) │     content/)           │ (render)    │     • skill  (agents)
                        └─────────┘                         └─────────────┘     • html
```

- **ingest** is agnostic to input format — it consumes text/markdown and runs an agent to write the content model. Turning an email, webpage, or video transcript into markdown is **upstream glue**, not agwiki's job: agwiki starts at a local artifact and never fetches from remote services (so there is no auth surface).
- **materialize** is agnostic to its consumer — it deterministically renders the content model into a `wiki`, `skill`, or `html` target.

It is strongly opinionated by design: no security model, no multi-tenancy, one right way per choice. See [`CONTEXT.md`](CONTEXT.md) for the glossary and [`docs/adr/`](docs/adr) for the decisions behind this design.

## Install

- **GitHub Releases:** [github.com/goagwiki/agwiki/releases](https://github.com/goagwiki/agwiki/releases) — download a binary for your platform.
- **Homebrew (macOS / Linux):** `brew install goagwiki/cli/agwiki` — see [goagwiki/homebrew-cli](https://github.com/goagwiki/homebrew-cli).
- **Scoop (Windows):** add the [goagwiki/scoop-bucket](https://github.com/goagwiki/scoop-bucket) and `scoop install agwiki`.

If you are changing this repository, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Quick start

Create a new wiki root (writes `agwiki.toml`, the directory tree, and `ingest.md` from the embedded template):

```bash
agwiki init ./my-wiki
```

Or use an existing content repo: it needs **`wiki/`**, and **`ingest`** requires **`ingest.md`** at the wiki root. You can copy from this repo's [`prompts/ingest.md`](crates/agwiki-core/prompts/ingest.md) if you did not use `init`.

Template placeholders: **`{{INGEST_PATH}}`** (absolute source `.md`), **`{{WIKI_ROOT}}`** (absolute wiki root).

```bash
agwiki ingest -a opencode ./raw/note.md       # ingest one source
agwiki materialize --target skill              # render the content model to a skill bundle
```

**`-C` / `--wiki-root`** is optional for `ingest`, `materialize`, `check`, and `serve`; when omitted, the current working directory is used (it must contain `wiki/` where required). Use `agwiki <command> --help` for flags and examples.

## Ingest

Ingest uses **aikit-sdk** (bundled) to run agents in-process, emitting **NDJSON lines on stdout**. No separate installation required. agwiki does **not** fetch or transcode sources — bring normalized text/markdown (your own glue, `yt-dlp`, a Gmail export, etc., produces it).

**Agent / model** resolve by precedence: flag → `AGWIKI_*` env var → `.agwiki/config.toml`:
`-a`/`--agent` → `AGWIKI_AGENT` → `[defaults].agent`, and `-m`/`--model` → `AGWIKI_MODEL` → `[defaults].model`. If nothing supplies an agent, ingest errors.

**Idempotency is always on.** Every successful ingest is recorded in the ledger `<wiki-root>/.agwiki/ingest-state.jsonl`, and an already-ingested source is **skipped** — so re-running a folder only ingests what's new. Identity is **external-id-authoritative**: a source carrying an `external_id` (via `--external-id` or YAML frontmatter `external_id:` — e.g. an email `Message-ID`, a YouTube `video_id`, a URL) is matched on `(wiki_root, external_id, ingest.md hash)`, ignoring content drift. Without an external id, content identity (source path + content hash + agent + model) is the fallback. Changing `ingest.md` re-ingests everything; **`--force`** re-ingests a single run.

**Plan before you pay.** `agwiki ingest --dry-run` resolves and validates sources and prints a JSON plan (`{"source":…,"action":"ingest"|"skip","reason":…,"external_id":…}`) **without** running the agent or writing the ledger.

### Operator settings — `.agwiki/config.toml`

Operator settings live in a git-ignored **`<wiki-root>/.agwiki/config.toml`**, kept separate from the committed `agwiki.toml` (wiki schema) and the `.agwiki/ingest-state.jsonl` ledger:

```toml
[defaults]
agent = "codex"
model = "gpt-5"          # optional

[hooks]                  # optional shell commands run at lifecycle points
after_source      = "echo ingested $AGWIKI_SOURCE_KEY"
after_batch       = "echo batch: $AGWIKI_INGESTED ingested, $AGWIKI_SKIPPED skipped"
after_materialize = "git -C $AGWIKI_WIKI_ROOT add -A && git commit -m 'materialize'"
on_error          = "echo failed: $AGWIKI_ERROR"
continue_on_error = false   # when true, a non-zero hook warns instead of failing the run
```

Hooks run via `sh -c` from the wiki root with `AGWIKI_*` env vars (`WIKI_ROOT`, `SOURCE`, `SOURCE_KEY`, `EXTERNAL_ID`, `AGENT`, `MODEL`, `TARGET`, batch counts, `ERROR`). A non-zero hook exit fails the command unless `continue_on_error = true`. Hooks never run under `--dry-run`. This replaces the bash glue (push/authorize) people chain after ingest.

## Commands

```text
agwiki init [DIR]
agwiki ingest [-C DIR] [-a NAME] [-m MODEL] [--stream] [--force] [--external-id ID] [--dry-run] [--ingest-state FILE] (<FILE> | --folder <DIR> [--max-files N])
agwiki materialize --target wiki  [-C DIR] [--dry-run]
agwiki materialize --target skill [-C DIR] [--skill-root DIR] [--skill-md FILE] [--dry-run] [--prune]
agwiki materialize --target html  [-C DIR] [--out DIR]
agwiki check wiki    [-C DIR] [--format text|json]
agwiki check sources [-C DIR]
agwiki serve         [-C DIR] [...]
```

- **`init`** — Create `DIR` (default `.`) if needed; `DIR` must be empty if it already exists. Writes `agwiki.toml`, creates configured subdirectories, and writes `ingest.md`.
- **`ingest`** — Resolve the source text file(s), load `<wiki-root>/ingest.md`, expand placeholders, run the agent via **aikit-sdk** with cwd set to the wiki root; stdout shows the NDJSON event stream, skip notices and batch summaries go to stderr. Idempotency is always on (see [Ingest](#ingest)); use `--force` to re-ingest, `--dry-run` to plan only, `--external-id`/frontmatter to set a stable id, `--folder` for batch.
- **`materialize`** — Render the content model into a concrete **target** layout (the wiki is just one target, not privileged). **`--target` is required** (no default): `wiki`, `skill`, or `html`. Target-specific flags are rejected when paired with the wrong target.
  - **`--target wiki`** — Validate content sources and render generated markdown into `wiki/`. `--dry-run` validates without writing.
  - **`--target skill`** — Mirror each immediate subdirectory of `wiki/` into `skill/references/<name>/`, build a markdown index from `wiki/index.md`, and update the block between `<!-- agwiki:generated-index -->` markers in `SKILL.md` (default `skill/SKILL.md`). Flags: `--skill-root`, `--skill-md`, `--dry-run`, `--prune`.
  - **`--target html`** — Static HTML export of the wiki into `--out` (default `dist/html`).
- **`check wiki`** — Report broken wikilinks/relative links and orphan pages under `wiki/`. Exit **1** on any problem. `--format text` (default) or `--format json`.
- **`check sources`** — Validate content sources without writing generated wiki files. Exit **1** on errors.

### `check wiki --format json` schema

Stable fields (paths are strings; `wiki_root` is absolute when canonicalization succeeds):

```json
{
  "wiki_root": "/path/to/wiki",
  "problems": [
    { "kind": "broken_link", "message": "…" },
    { "kind": "orphan", "message": "wiki/relative/path.md" }
  ]
}
```

An empty `problems` array means the wiki passed validation.

## Workspace & embedding

agwiki is a Cargo workspace:

- **`agwiki-core`** — the pure pipeline library (ingest, content model, materialize, the ledger). Emits an `IngestEvent` stream through a caller-supplied sink; reads no config and writes nothing to stdout, so it embeds cleanly.
- **`agwiki`** — the CLI (this binary) on top of cli-framework; owns config, hooks, the HTTP browse server (`serve`), and NDJSON rendering.
- **`agwiki-py`** — a [PyO3](https://pyo3.rs) binding exposing the pipeline to Python (`ingest_file`, `materialize`, `check_wiki`). Build with [maturin](https://www.maturin.rs); see [`crates/agwiki-py/README.md`](crates/agwiki-py/README.md).

## License

Apache-2.0
