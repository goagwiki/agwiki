# Agent Wiki

**Agent-based wiki** — a Rust CLI for markdown wikis: resolve ingest paths, run agent-driven ingest, check wikilinks, list orphan pages, and export an [Agent Skill](https://agentskills.io/) bundle from your wiki.

## Install

Build from this repository:

```bash
cargo install --path /path/to/agwiki
```

Or use release binaries from GitHub when published.

## Prerequisites

- **Ingest:** `aikit` (default) or `opencode` on `PATH`, depending on `--runner`.
- Content wiki layout: `wiki/` under the wiki root; optional `raw/`, `skill/`, etc.

agwiki does **not** handle PDF or YouTube; use other tools for those.

## Environment

| Variable | Purpose |
|----------|---------|
| `WIKI_ROOT` | Default wiki root (`--wiki-root` / `-C` override). |
| `AGWIKI_ROOT` | Toolkit checkout (this repo or install): `prompts/ingest.md`, `AGENTS.md`. Overrides `FASTWIKI_ROOT` / `WIKIFY_ROOT`. |
| `FASTWIKI_ROOT` | Same as `AGWIKI_ROOT` if unset. |
| `WIKIFY_ROOT` | Same if the others are unset. |
| `WIKI_INGEST_PROMPT` | Optional path to a custom ingest prompt template. |

## Commands

```text
agwiki prep [--wiki-root DIR | -C DIR] [--raw-only] <FILE.md>
agwiki ingest [--wiki-root DIR | -C DIR] [--runner aikit|opencode] <FILE.md>
agwiki check-links [--wiki-root DIR | -C DIR]
agwiki orphans [--wiki-root DIR | -C DIR]
agwiki export-skill [--wiki-root DIR | -C DIR] [--skill-dir PATH] [--template PATH] [--output PATH] [--index PATH] [--sections STR] [--dry-run] [--prune] [--rewrite-wikilinks]
```

- **`prep`** — Print the absolute path to the `.md` source; `--raw-only` requires the file under `<wiki-root>/raw/`.
- **`ingest`** — Expand the ingest prompt (toolkit + placeholders) and run the agent with cwd = wiki root.
- **`check-links`** — Scan `wiki/**/*.md` for broken wikilinks and relative links; exits non-zero if any.
- **`orphans`** — List wiki pages with no incoming wikilink (entry pages like `wiki/index.md` skipped).
- **`export-skill`** — Mirror `wiki/<sections>/` into `skill/references/` and write `SKILL.md` from `SKILL.md.template` plus an index section from `wiki/index.md`.

## Local checks

```bash
./scripts/run-tests.sh
```

## License

Apache-2.0
