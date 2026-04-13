---
name: agwiki
description: Operates the agwiki CLI for agent-driven markdown wikis — scaffold with init, ingest raw notes via aikit-sdk, validate wikilinks and orphans, and export-skill bundles aligned with agentskills.io. Use when the user mentions agwiki, agent wiki, wiki ingest, export-skill, agwiki.toml, ingest.md, wiki/index.md, skill/references, broken wikilinks, or orphan wiki pages.
compatibility: Requires agwiki ≥ 0.1.11 (GitHub Releases, Homebrew goagwiki/cli/agwiki, or Scoop goagwiki/scoop-bucket). Command ingest runs an aikit-sdk agent in-process; -a/--agent is required (no default). --folder batch mode added in 0.1.11.
metadata:
  version: "1.1.0"
license: Apache-2.0
---

# agwiki CLI (tools-agwiki)

[Agent Wiki](https://github.com/goagwiki/agwiki) is a Rust CLI: **`init`** scaffolds a wiki repo, **`ingest`** runs an embedded agent against `ingest.md`, **`validate`** fails on broken links and orphans, **`export-skill`** mirrors `wiki/` into an Agent Skill layout, and **`serve`** starts a local browser UI. Format follows [Agent Skills](https://agentskills.io/).

This skill ships inside the agwiki repository under `skill/tools-agwiki/`. To hack the tool itself, use the repo root and `cargo build` / `cargo test`.

## Repository contract

- Wiki root must contain **`wiki/`**.
- **`ingest`** requires **`ingest.md`** at the wiki root. The CLI expands **`{{INGEST_PATH}}`** (absolute path to the source `.md`) and **`{{WIKI_ROOT}}`** (absolute wiki root) in that file before running the agent. If `ingest.md` is missing, ingest errors. Copy from upstream `prompts/ingest.md` when adopting an existing repo without `init`.
- **`agwiki init`** writes **`agwiki.toml`**, creates **`raw/`**, **`templates/`**, **`skill/`**, wiki subtrees (`wiki/concepts`, `topics`, `sources`, `projects`, `people`, `syntheses`), and starter **`wiki/index.md`**, **`wiki/inbox.md`**, **`wiki/log.md`**, plus **`ingest.md`**.

## Commands

| Command | Role |
|---------|------|
| **`agwiki init [DIR]`** | Create wiki root at `DIR` (default `.`). **Fails if `DIR` exists and is not empty.** |
| **`agwiki ingest -a <AGENT> <FILE>`** | Ingest a **single** source file (any UTF-8 text, not just `.md`); **`-a` / `--agent` required**. Optional **`-C`**, **`-m`**, **`--stream`**. Conflicts with `--folder`. |
| **`agwiki ingest -a <AGENT> --folder <DIR>`** | **Batch mode** (≥ 0.1.11): ingest all `*.md` files under `DIR` recursively. Optional **`--max-files`** (default `30`, `0` = unlimited). Conflicts with `<FILE>`. |
| **`agwiki validate`** | Broken wikilinks, relative markdown links, orphan pages (entry pages like **`wiki/index.md`** skipped). Exits **1** if any problem. **`--format text`** (default) or **`json`**. Optional **`-C` / `--wiki-root`**. |
| **`agwiki export-skill`** | For each **immediate subdirectory** of **`wiki/`**, copies `wiki/<name>/**/*.md` → **`skill/references/<name>/`**. Requires **`wiki/index.md`**. Updates **`SKILL.md`** inside **`<!-- agwiki:generated-index -->`** … **`<!-- /agwiki:generated-index -->`** markers. Optional **`-C`**, **`--skill-root`**, **`--skill-md`**, **`--dry-run`**, **`--prune`**. |
| **`agwiki serve`** | Local HTTP UI for the wiki. Optional **`-C`**, **`--port`** (default `8080`), **`--host`** (default `127.0.0.1`), **`--open`**. |

Omitting **`-C` / `--wiki-root`**: cwd must be the wiki root (contain **`wiki/`**).

## Command reference

### `agwiki init [DIR]`

```
agwiki init               # scaffold wiki in current directory
agwiki init ./my-wiki     # scaffold into a new subdirectory
```

Fails if the target directory exists and is not empty.

### `agwiki ingest` — single file

```
agwiki ingest -a opencode ./raw/note.md
agwiki ingest -a claude ./raw/note.md
agwiki ingest -C /path/to/wiki -a claude ./raw/note.md
agwiki ingest --stream -a opencode ./raw/note.md
agwiki ingest -a opencode -m <MODEL> ./raw/note.md
```

| Flag | Description |
|------|-------------|
| `<FILE>` | Source text file (resolved from cwd; UTF-8, no null bytes). Conflicts with `--folder`. |
| `-a` / `--agent <NAME>` | aikit-sdk agent key (`opencode`, `claude`, `codex`, `gemini`, …). **Required.** |
| `-C` / `--wiki-root <DIR>` | Wiki root (default: cwd). |
| `-m` / `--model <MODEL>` | Model override passed to aikit-sdk. |
| `--stream` | Enable agent-native streaming where supported. |

`<FILE>` accepts any UTF-8 text file (`.md`, `.txt`, `.json`, `.yaml`, `.log`, no extension, etc.) — extension is not checked.

### `agwiki ingest --folder` — batch mode (≥ 0.1.11)

```
agwiki ingest -a opencode --folder ./raw
agwiki ingest -a claude --folder ./raw --max-files 0
agwiki ingest -a opencode --folder ./raw --max-files 10
agwiki ingest -C /path/to/wiki -a claude --folder ./raw
agwiki ingest --stream -a opencode --folder ./raw
```

| Flag | Description |
|------|-------------|
| `--folder <DIR>` | Discover and ingest all `*.md` / `*.MD` files under `DIR` recursively (no symlinks, sorted lexicographically). Conflicts with `<FILE>`. |
| `--max-files <N>` | Cap on files to ingest (default: `30`; `0` = unlimited). Errors **before** ingesting any file if the count is exceeded. |
| `-a` / `--agent <NAME>` | aikit-sdk agent key. **Required.** |
| `-C` / `--wiki-root <DIR>` | Wiki root (default: cwd). |
| `-m` / `--model <MODEL>` | Model override passed to aikit-sdk. |
| `--stream` | Enable agent-native streaming where supported. |

**Batch behaviour:** Continues through all discovered files even if individual ones fail. Prints a summary to **stderr** (`Batch ingest: X total, Y succeeded, Z failed.`) and lists each failure. Exits **1** if any file failed.

### `agwiki validate`

```
agwiki validate
agwiki validate -C /path/to/wiki
agwiki validate --format json
```

| Flag | Description |
|------|-------------|
| `-C` / `--wiki-root <DIR>` | Wiki root (default: cwd). |
| `--format <FORMAT>` | `text` (default) or `json`. |

Exits **1** if any broken link or orphan is found. Use in CI as the failing gate.

### `agwiki export-skill`

```
agwiki export-skill
agwiki export-skill --prune
agwiki export-skill -C /path/to/wiki --dry-run
agwiki export-skill --skill-root ./my-skill
agwiki export-skill --skill-md ./my-skill/SKILL.md
```

| Flag | Description |
|------|-------------|
| `-C` / `--wiki-root <DIR>` | Wiki root (default: cwd). |
| `--skill-root <DIR>` | Agent Skill directory (default: `<wiki-root>/skill`). |
| `--skill-md <FILE>` | SKILL.md path to create or update (default: `<skill-root>/SKILL.md`). |
| `--dry-run` | Print planned copies/prunes and generated index; do not write files. |
| `--prune` | Remove files under `skill/references/` when the source `.md` no longer exists in the wiki. |

Exits **0** even when validation warnings appear on stderr — use `agwiki validate` in CI for a hard failure.

### `agwiki serve`

```
agwiki serve
agwiki serve --open
agwiki serve --port 8081
agwiki serve --host 0.0.0.0 --port 8080
agwiki serve -C /path/to/wiki --open
```

| Flag | Description |
|------|-------------|
| `-C` / `--wiki-root <DIR>` | Wiki root (default: cwd). |
| `--port <PORT>` | Port to listen on (default: `8080`). |
| `--host <HOST>` | Host/IP to bind (default: `127.0.0.1`). |
| `--open` | Automatically open the wiki in the default browser. |

## Workflows

**New wiki:** `agwiki init <dir>` → edit under **`wiki/`** → `agwiki validate` → optional `agwiki export-skill`.

**Ingest a raw note:** Place or keep the source under **`raw/`** (agents must **not** edit **`raw/`**). Run `agwiki ingest -a <agent> ./raw/note.md` from a cwd where the path resolves. The agent follows rules in **`ingest.md`**: work only under **`wiki/`**, prefer updating existing pages, link related pages, keep **`wiki/index.md`** current, append to **`wiki/log.md`**, use **`wiki/sources/`** and `templates/source-page.md` per the embedded workflow. For the full rule text, read **`ingest.md`** in the wiki root (or upstream `prompts/ingest.md`).

**Batch ingest (≥ 0.1.11):** `agwiki ingest -a opencode --folder ./raw` — discovers all `*.md` recursively, default cap of 30 files (pass `--max-files 0` for unlimited).

**Publish a skill bundle:** Keep **`wiki/index.md`** wikilinks accurate so the generated index matches intent → `agwiki export-skill --dry-run` to preview → `agwiki export-skill` (add **`--prune`** when wiki pages were removed so stale `skill/references/**` files drop).

**CI pipeline:**
```
agwiki validate           # fails on broken links / orphans
agwiki export-skill       # update skill bundle
```

## HTML comment markers (export-skill)

Use **exactly** this opening line:

`<!-- agwiki:generated-index -->`

and this closing line:

`<!-- /agwiki:generated-index -->`

**Pitfalls:** A start marker **without** a matching end (or an end **without** a start) makes **`export-skill`** error when merging the generated index. Only **top-level directories directly under `wiki/`** become **`skill/references/<dir>/`**; nested structure under each dir is preserved.

## Validation JSON

For machine-readable validate output, see [references/validate-json-schema.md](references/validate-json-schema.md). Summary: top-level **`wiki_root`**, **`problems`** array; each problem has **`kind`** (`broken_link` or `orphan`) and a **`message`**. Empty **`problems`** means pass.

## Limitations

- agwiki does **not** ingest PDF or YouTube; use other tools for those media.
- `ingest --folder` batch mode requires **≥ 0.1.11**; on 0.1.10 use a shell loop instead.
- `--agent` has no default value and must always be specified explicitly.
- `--folder` and `<FILE>` cannot be combined; choose one.

<!-- agwiki:generated-index -->
<!-- /agwiki:generated-index -->
