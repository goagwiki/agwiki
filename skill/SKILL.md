---
name: agwiki
description: Operates the agwiki CLI for agent-driven markdown wikis — scaffold with init, add sources with new, ingest notes via aikit-sdk (idempotent, external-id aware), check wikilinks and orphans, materialize the content model into wiki/skill/html targets, and browse with serve. Use when the user mentions agwiki, agent wiki, wiki ingest, materialize, .agwiki/config.toml, ingest.md, wiki/index.md, broken wikilinks, or orphan wiki pages.
compatibility: Requires agwiki ≥ 0.3.12 (GitHub Releases, Homebrew goagwiki/cli/agwiki, or Scoop goagwiki/scoop-bucket). ingest runs an aikit-sdk agent in-process; the agent resolves from -a/--agent, then AGWIKI_AGENT, then .agwiki/config.toml. Idempotency is always on (no --resume flag). compile/export were replaced by `materialize --target`.
metadata:
  version: "2.0.0"
license: Apache-2.0
---

# agwiki CLI

[Agent Wiki](https://github.com/goagwiki/agwiki) is a Rust CLI that runs an agent-driven wiki pipeline: **`ingest`** turns sources into a structured content model, then **`materialize`** renders that model into a browsable wiki, an [Agent Skill](https://agentskills.io/) bundle, or static HTML. **`check`** fails on broken links and orphans, and **`serve`** opens a local browser UI.

agwiki does **not** fetch or transcode sources — bring normalized text/markdown (your own export, `yt-dlp`, a Gmail export, etc.). It starts from a local file.

## Repository contract

- The wiki root must contain **`wiki/`**.
- **`ingest`** requires **`ingest.md`** at the wiki root. The CLI expands **`{{INGEST_PATH}}`** (absolute path to the source file) and **`{{WIKI_ROOT}}`** (absolute wiki root) in it before running the agent. If `ingest.md` is missing, ingest errors. Copy from upstream `prompts/ingest.md` when adopting an existing repo without `init`.
- **`agwiki init`** writes **`agwiki.toml`**, creates **`raw/`**, **`templates/`**, **`skill/`**, the wiki subtrees (`wiki/concepts`, `topics`, `sources`, `projects`, `people`, `syntheses`), starter **`wiki/index.md`**, **`wiki/inbox.md`**, **`wiki/log.md`**, and **`ingest.md`**.

## Commands

| Command | Role |
|---------|------|
| **`agwiki init [DIR]`** | Create a wiki root at `DIR` (default `.`). **Fails if `DIR` exists and is not empty.** |
| **`agwiki new <KIND>`** | Scaffold a content source file under **`content/<KIND>/`** (e.g. `concepts`). Optional **`--title`**, **`-C`**. |
| **`agwiki ingest [-a AGENT] <FILE>`** | Ingest a **single** source file (any UTF-8 text). Idempotency is always on. Optional **`-C`**, **`-m`**, **`--stream`**, **`--progress`**, **`--force`**, **`--external-id`**, **`--dry-run`**, **`--compile`**, **`--ingest-state`**. Conflicts with `--folder`. |
| **`agwiki ingest [-a AGENT] --folder <DIR>`** | **Batch mode**: ingest all `*.md` under `DIR` recursively. Optional **`--max-files`** (default `30`, `0` = unlimited). Conflicts with `<FILE>`. |
| **`agwiki materialize --target <T>`** | Render the content model into a target layout: **`wiki`**, **`skill`**, or **`html`**. **`--target` is required.** |
| **`agwiki check wiki`** | Broken wikilinks, relative links, orphan pages. Exits **1** on any problem. **`--format text`** (default) or **`json`**. |
| **`agwiki check sources`** | Validate content sources without writing wiki files. Exits **1** on errors. |
| **`agwiki serve`** | Local HTTP UI for the wiki. Optional **`-C`**, **`--port`**, **`--host`**, **`--open`**. |

Omitting **`-C` / `--wiki-root`**: cwd must be the wiki root (contain **`wiki/`**).

## Ingest

The **agent** resolves by precedence: **`-a`/`--agent`** → **`AGWIKI_AGENT`** env var → **`[defaults].agent`** in `.agwiki/config.toml`. The **model** follows the same precedence (`-m`/`--model` → `AGWIKI_MODEL` → `[defaults].model`). If nothing supplies an agent, ingest errors.

**Idempotency is always on.** Each successful ingest is recorded in **`<wiki-root>/.agwiki/ingest-state.jsonl`**, and an already-ingested source is **skipped** — re-running a folder only ingests what is new. Identity is **external-id-authoritative**: a source carrying an `external_id` (from **`--external-id`** or YAML frontmatter `external_id:` — e.g. an email `Message-ID`, a video id, a URL) is matched on `(wiki_root, external_id, ingest.md hash)`, ignoring content changes. Without an external id, content identity (source path + content hash + agent + model) is the fallback. Changing `ingest.md` re-ingests everything; **`--force`** re-ingests one run.

**`--dry-run`** resolves and validates sources and prints a JSON plan (`{"source":…,"action":"ingest"|"skip","reason":…,"external_id":…}`) **without** running the agent or writing the ledger.

```
agwiki ingest -a opencode ./raw/note.md
agwiki ingest ./raw/note.md                          # agent from .agwiki/config.toml
agwiki ingest -a codex --external-id vid-123 ./raw/note.md
agwiki ingest -a codex --dry-run --folder ./raw      # preview, no agent run
agwiki ingest -a codex --force ./raw/note.md         # re-ingest a seen source
agwiki ingest -a opencode --folder ./raw --max-files 0
```

| Flag | Description |
|------|-------------|
| `<FILE>` | Source text file (UTF-8, no null bytes; any extension). Conflicts with `--folder`. |
| `--folder <DIR>` | Ingest all `*.md` / `*.MD` under `DIR` recursively (no symlinks, sorted). Conflicts with `<FILE>`. |
| `--max-files <N>` | Cap for `--folder` (default `30`; `0` = unlimited). Errors **before** ingesting if exceeded. |
| `-a` / `--agent <NAME>` | aikit-sdk agent key (`opencode`, `claude`, `codex`, `gemini`, …). Falls back to `AGWIKI_AGENT`, then config. |
| `-m` / `--model <MODEL>` | Model override (falls back to `AGWIKI_MODEL`, then config). |
| `--external-id <ID>` | Stable id for this source (overrides frontmatter `external_id`; single-file only). |
| `--force` | Re-ingest even when the ledger holds a matching success record. |
| `--dry-run` | Emit a JSON plan without running the agent or writing the ledger. |
| `--compile` | Run `materialize --target wiki` after a successful ingest. |
| `--stream` | Enable agent-native streaming where supported. |
| `--progress` | Render a live progress view on stderr instead of NDJSON. |
| `--ingest-state <FILE>` | Ledger path (default `<wiki-root>/.agwiki/ingest-state.jsonl`; relative paths resolve under the wiki root). |
| `-C` / `--wiki-root <DIR>` | Wiki root (default: cwd). |

**Batch behaviour:** continues through all files even if some fail; prints a stderr summary (`Batch ingest: X total, Y succeeded, Z skipped, W failed.`) and lists failures; exits **1** if any file failed.

## Operator settings — `.agwiki/config.toml`

A git-ignored **`<wiki-root>/.agwiki/config.toml`** holds operator defaults and lifecycle hooks (kept separate from the committed `agwiki.toml` schema and the `.agwiki/ingest-state.jsonl` ledger):

```toml
[defaults]
agent = "codex"
model = "gpt-5"          # optional

[hooks]                  # optional; shell commands run via `sh -c` from the wiki root
after_source      = "echo ingested $AGWIKI_SOURCE_KEY"
after_batch       = "echo $AGWIKI_INGESTED ingested, $AGWIKI_SKIPPED skipped"
after_materialize = "git -C $AGWIKI_WIKI_ROOT add -A && git commit -m materialize"
on_error          = "echo failed: $AGWIKI_ERROR"
continue_on_error = false
```

Hooks receive `AGWIKI_*` env vars (`WIKI_ROOT`, `SOURCE`, `SOURCE_KEY`, `EXTERNAL_ID`, `AGENT`, `MODEL`, `TARGET`, batch counts, `ERROR`). A non-zero hook exit fails the command unless `continue_on_error = true`. Hooks never run under `--dry-run`.

## Materialize

`materialize` renders the content model into a required **`--target`** layout. Target-specific flags are rejected when paired with the wrong target.

```
agwiki materialize --target wiki                       # render content/ into wiki/
agwiki materialize --target wiki --dry-run             # validate without writing
agwiki materialize --target skill --dry-run            # preview the skill bundle
agwiki materialize --target skill --prune              # drop stale skill/references/**
agwiki materialize --target html --out ./dist/html     # static HTML export
```

| Target | Flags | Behaviour |
|--------|-------|-----------|
| **`wiki`** | `--dry-run` | Validate content sources and render generated markdown into `wiki/`. |
| **`skill`** | `--skill-root`, `--skill-md`, `--dry-run`, `--prune` | For each immediate subdirectory of `wiki/`, mirror `wiki/<name>/**/*.md` → `skill/references/<name>/`; build a markdown index from `wiki/index.md`; update the block between `<!-- agwiki:generated-index -->` and `<!-- /agwiki:generated-index -->` in `SKILL.md` (default `skill/SKILL.md`). |
| **`html`** | `--out` | Static HTML export of the wiki into `--out` (default `dist/html`). |

`materialize --target skill` runs the same checks as `check wiki` afterward and prints **warnings on stderr** but still exits **0** — use `agwiki check wiki` in CI for a failing exit code.

## Check & serve

```
agwiki check wiki                 # broken links + orphans; exit 1 on any problem
agwiki check wiki --format json   # machine-readable (see references/validate-json-schema.md)
agwiki check sources              # validate content sources only
agwiki serve --open               # browse the wiki locally
```

`check wiki` flags: `-C`, `--format text|json`. `serve` flags: `-C`, `--port` (default `8080`), `--host` (default `127.0.0.1`), `--open`.

## Workflows

**New wiki:** `agwiki init <dir>` → add sources with `agwiki new <kind>` and edit under `content/` → `agwiki materialize --target wiki` → `agwiki check wiki`.

**Ingest a raw note:** keep the source under **`raw/`** (agents must **not** edit `raw/`). Run `agwiki ingest -a <agent> ./raw/note.md`. The agent follows `ingest.md`: work only under `wiki/`, prefer updating existing pages, link related pages, keep `wiki/index.md` current, append to `wiki/log.md`. Re-running is safe — already-ingested sources are skipped.

**Bulk ingest:** stamp each prepared source's frontmatter with `external_id:` (or pass `--external-id`), then `agwiki ingest -a codex --folder ./raw --max-files 0`. Preview first with `--dry-run`. Re-runs only ingest new ids.

**Publish a skill bundle:** keep `wiki/index.md` wikilinks accurate → `agwiki materialize --target skill --dry-run` to preview → `agwiki materialize --target skill` (add `--prune` to drop stale references).

**CI gate:**
```
agwiki check wiki                    # fails on broken links / orphans
agwiki materialize --target skill    # refresh the skill bundle
```

## HTML comment markers (materialize --target skill)

Use **exactly** these lines around the generated index in `SKILL.md`:

`<!-- agwiki:generated-index -->` … `<!-- /agwiki:generated-index -->`

A start marker without a matching end (or vice versa) makes `materialize --target skill` error. Only top-level directories directly under `wiki/` become `skill/references/<dir>/`; nested structure is preserved.

## Validation JSON

See [references/validate-json-schema.md](references/validate-json-schema.md). Top-level **`wiki_root`** and a **`problems`** array; each problem has **`kind`** (`broken_link` or `orphan`) and a **`message`**. Empty **`problems`** means pass.

## Limitations

- agwiki does **not** fetch or transcode sources — it starts from a local UTF-8 text file. Convert email/web/video to markdown with other tools first.
- `--folder` and `<FILE>` cannot be combined; choose one.
- `--external-id` applies to single-file ingest; in `--folder` mode each file's id comes from its own frontmatter.
- There is no security or multi-tenancy model — agwiki is for a single user or a small trusted group.

<!-- agwiki:generated-index -->
<!-- /agwiki:generated-index -->
