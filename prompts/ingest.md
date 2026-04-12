Ingest {{INGEST_PATH}} into the wiki at {{WIKI_ROOT}}.

Follow the embedded default wiki rules below (folder layout, rules, ingest steps). Work only under `{{WIKI_ROOT}}`; do not require reading paths outside that wiki root.

# Purpose

Default rules for markdown content wikis consumed by **wikify** (`wiki-ingest`). Paths are relative to the wiki root (`WIKI_ROOT`).

# Rules

- Never modify files inside `raw/`
- Treat `raw/` as source material
- Create and update pages only inside `wiki/`
- Prefer updating existing pages instead of creating duplicates
- Preserve uncertainty and disagreements between sources
- Use concise markdown
- Add internal links between related pages
- Keep `wiki/index.md` updated
- Append important actions to `wiki/log.md`

# Folder meanings

- `raw/` — original source documents (optional subfolders such as notes, articles, tweets; paths are stable identifiers)
- `templates/` — copy `source-page.md` when creating a new `wiki/sources/` page
- `wiki/sources/` — summaries of individual sources
- `wiki/concepts/` — concept pages
- `wiki/projects/` — project pages
- `wiki/people/` — person pages
- `wiki/topics/` — broader topic pages
- `wiki/syntheses/` — higher-level combined understanding
- `wiki/inbox.md` — temporary capture / unprocessed ideas

# Ingest workflow

When asked to ingest a file from `raw/`:

1. Read the source
2. Create or update a source summary in `wiki/sources/` (start from `templates/source-page.md`; mirror raw path in filenames, e.g. `raw/tweets/x.md` → `wiki/sources/tweets/x.md`)
3. Add a wikilink under **Ingested** in `wiki/sources/index.md`
4. Update related concept/topic/project/person pages
5. Add links to related pages
6. Update `wiki/index.md` when navigation or top-level structure changes
7. Append a short note to `wiki/log.md`

# Query workflow

When asked a question:

1. Start from `wiki/index.md`
2. Read relevant wiki pages
3. Answer using the wiki
4. If useful, create or improve a reusable wiki page

# Maintenance workflow

When asked to clean or lint the wiki:

- find duplicates
- find contradictions
- find orphan pages
- find stale pages
- suggest missing pages

Use concise markdown; prefer updating existing pages over duplicates; preserve uncertainty between sources.
