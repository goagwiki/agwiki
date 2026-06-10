# Agent Wiki

**Agent-based wiki** — a CLI for markdown wikis: scaffold a repo (`init`), agent-driven ingest, unified validation (broken links and orphan pages), and export an [Agent Skill](https://agentskills.io/) bundle from your wiki.

## Install

- **GitHub Releases:** [github.com/goagwiki/agwiki/releases](https://github.com/goagwiki/agwiki/releases) — download a binary for your platform.
- **Homebrew (macOS / Linux):** `brew install goagwiki/cli/agwiki` — see [goagwiki/homebrew-cli](https://github.com/goagwiki/homebrew-cli).
- **Scoop (Windows):** add the [goagwiki/scoop-bucket](https://github.com/goagwiki/scoop-bucket) and `scoop install agwiki`.

If you are changing this repository, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Quick start

Create a new wiki root (writes `agwiki.toml`, directory tree, and `ingest.md` from the embedded template):

```bash
agwiki init ./my-wiki
```

Or use an existing content repo: it needs **`wiki/`**, and **`ingest`** requires **`ingest.md`** at the wiki root. You can copy from this repo’s [`prompts/ingest.md`](prompts/ingest.md) if you did not use `init`. If that file is missing, `agwiki ingest` exits with an error.

Template placeholders: **`{{INGEST_PATH}}`** (absolute source `.md`), **`{{WIKI_ROOT}}`** (absolute wiki root).

```bash
agwiki ingest -a opencode ./raw/note.md
```

Resume mode (opt-in) records successful ingests to a local ledger and skips re-ingesting sources that already succeeded under the same identity (wiki root + source key + source content hash + `ingest.md` hash + agent + model):

```bash
agwiki ingest --resume -a codex ./raw/note.md
```

**`-C` / `--wiki-root`** is optional for `ingest`, `compile`, `export`, `check`, and `serve`; when omitted, the current working directory is used (it must contain `wiki/` where required). Use `agwiki <command> --help` for flags and examples.

## What you need to use ingest

- No separate installation required. Ingest uses **aikit-sdk** (bundled as a Cargo dependency) to run agents directly in-process, emitting **NDJSON lines on stdout** via an SDK event callback. The **agent** is resolved by precedence: **`-a` / `--agent`** flag → **`AGWIKI_AGENT`** env var → **`[defaults].agent`** in `.agwiki/config.toml` (see below). If none provides one, ingest exits with an error. The **model** follows the same precedence (`-m` / `--model` → `AGWIKI_MODEL` → `[defaults].model`); `--stream` is optional.

### Operator settings — `.agwiki/config.toml`

Operator defaults live in a git-ignored **`<wiki-root>/.agwiki/config.toml`**, kept separate from the committed `agwiki.toml` (wiki schema) and the `.agwiki/ingest-state.jsonl` ledger. It lets you avoid repeating `-a`/`-m` on every run:

```toml
[defaults]
agent = "codex"
model = "gpt-5"   # optional
```

With this in place, `agwiki ingest ./raw/note.md` runs without `-a`. Flags and `AGWIKI_*` env vars still override the file.

agwiki does **not** handle PDF or YouTube; use other tools for those.

## Commands

```text
agwiki init [DIR]
agwiki ingest [-C DIR] [-a NAME] [-m MODEL] [--stream] [--resume [--force] [--ingest-state FILE]] (<FILE> | --folder <DIR> [--max-files N])
agwiki check wiki [-C DIR] [--format text|json]
agwiki check sources [-C DIR]
agwiki export skill [-C DIR] [--skill-root DIR] [--skill-md FILE] [--dry-run] [--prune]
agwiki export html [-C DIR] [--out DIR]
```

- **`init`** — Create `DIR` (default `.`) if needed; `DIR` must be empty if it already exists. Writes `agwiki.toml`, creates configured subdirectories, and writes `ingest.md`.
- **`ingest`** — Resolve the source text file (must exist, from cwd), load `<wiki-root>/ingest.md`, expand placeholders, run the agent via **aikit-sdk** with cwd set to the wiki root; stdout shows the NDJSON event stream from the SDK callback. With `--resume`, successful ingests are appended to `<wiki-root>/.agwiki/ingest-state.jsonl` (or `--ingest-state FILE`) and subsequent runs skip sources that match the same identity; skip notices and batch summaries are printed to stderr.
- **`check wiki`** — Report broken wikilinks and relative markdown links under `wiki/`, and list orphan pages (no incoming wikilink; entry pages such as `wiki/index.md` are skipped). Exits with status **1** if there is any problem. **`--format text`** (default) prints human-readable sections; **`--format json`** prints a single JSON object (see below).
- **`check sources`** — Validate ontology content sources without writing generated wiki files (dry-run compile). Exits with status **1** if compilation finds any errors.
- **`export skill`** — For each **immediate subdirectory** of `wiki/`, mirror `wiki/<name>/**/*.md` into `skill/references/<name>/`. Reads **`wiki/index.md`** to build a markdown index of links into those files. Updates **`SKILL.md`** (default `skill/SKILL.md`) by **replacing** the block between `<!-- agwiki:generated-index -->` and `<!-- /agwiki:generated-index -->`, or **appending** that block (including the markers) if those lines are not present yet. There is no separate template file. After export, runs the same checks as **`check wiki`** and prints **warnings on stderr** if anything is wrong; the command still exits **0** (use **`agwiki check wiki`** in CI for a failing exit code).

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

## License

Apache-2.0
