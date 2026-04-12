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

**`-C` / `--wiki-root`** is optional for `ingest`, `validate`, and `export-skill`; when omitted, the current working directory is used (it must contain `wiki/`). Use `agwiki <command> --help` for flags and examples.

## What you need to use ingest

- **`aikit`** on your `PATH`. Ingest runs **`aikit run --events`** so progress is emitted as **NDJSON lines on stdout** (see **`aikit run --help`**). **`-a` / `--agent` is required** (no default). Optional **`-m` / `--model`** and **`--stream`** as for `aikit run`.

agwiki does **not** handle PDF or YouTube; use other tools for those.

## Commands

```text
agwiki init [DIR]
agwiki ingest [-C DIR] -a NAME [-m MODEL] [--stream] <FILE.md>
agwiki validate [-C DIR] [--format text|json]
agwiki export-skill [-C DIR] [--skill-root DIR] [--skill-md FILE] [--dry-run] [--prune]
```

- **`init`** — Create `DIR` (default `.`) if needed; `DIR` must be empty if it already exists. Writes `agwiki.toml`, creates configured subdirectories, and writes `ingest.md`.
- **`ingest`** — Resolve the source `.md` (must exist, from cwd), load `<wiki-root>/ingest.md`, expand placeholders, run **`aikit run --events`** with cwd set to the wiki root; stdout shows the event stream for monitoring.
- **`validate`** — Report broken wikilinks and relative markdown links under `wiki/`, and list orphan pages (no incoming wikilink; entry pages such as `wiki/index.md` are skipped). Exits with status **1** if there is any problem. **`--format text`** (default) prints human-readable sections; **`--format json`** prints a single JSON object (see below).
- **`export-skill`** — For each **immediate subdirectory** of `wiki/`, mirror `wiki/<name>/**/*.md` into `skill/references/<name>/`. Reads **`wiki/index.md`** to build a markdown index of links into those files. Updates **`SKILL.md`** (default `skill/SKILL.md`) by **replacing** the block between `<!-- agwiki:generated-index -->` and `<!-- /agwiki:generated-index -->`, or **appending** that block (including the markers) if those lines are not present yet. There is no separate template file. After export, runs the same checks as **`validate`** and prints **warnings on stderr** if anything is wrong; the command still exits **0** (use **`agwiki validate`** in CI for a failing exit code).

### `validate --format json` schema

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
