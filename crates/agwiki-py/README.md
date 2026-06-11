# agwiki-py

Python bindings for [`agwiki-core`](../agwiki-core). A PyO3 extension module
(`agwiki`) that exposes the agwiki pipeline to Python so a script can ingest raw
notes, materialize the content model into a target layout, and check a wiki —
all in-process, without shelling out to the `agwiki` CLI.

## Build

This is a [maturin](https://www.maturin.rs/) extension module. From this
directory:

```bash
# Develop into the active virtualenv (recommended for local use)
maturin develop

# …or build a wheel
maturin build --release
```

If you do not have a virtualenv active, create one first:

```bash
python3 -m venv .venv && . .venv/bin/activate && pip install maturin
maturin develop
```

The module is built against the stable ABI (`abi3-py39`), so a single wheel
works on CPython 3.9 and newer.

## Usage

```python
import agwiki

# 1. Ingest a single source file. The ingest ledger lives at
#    <wiki_root>/.agwiki/ingest-state.jsonl and the prompt at <wiki_root>/ingest.md.
def on_event(ev):
    print(ev["type"], ev.get("source_key", ""))

result = agwiki.ingest_file(
    wiki_root="/path/to/wiki",
    source="raw/note.md",
    agent="codex",
    model=None,            # optional model override
    external_id=None,      # optional stable identity override
    force=False,           # re-ingest even if already seen
    dry_run=False,         # plan only; no agent run, no ledger write
    on_event=on_event,     # optional callable receiving each event as a dict
)
# -> {"outcome": "ingested" | "skipped", "source_key": ..., "external_id": ...}

# 2. Materialize the content model into a target layout: "wiki", "skill", or "html".
agwiki.materialize(wiki_root="/path/to/wiki", target="wiki")
agwiki.materialize(wiki_root="/path/to/wiki", target="skill", prune=True)
agwiki.materialize(wiki_root="/path/to/wiki", target="html", out="dist/html")
# Returns None on success; raises RuntimeError on error.

# 3. Check a wiki for broken wikilinks and orphan pages.
problems = agwiki.check_wiki(wiki_root="/path/to/wiki")
# -> [] when clean, else [{"kind": "broken_link" | "orphan", "message": ...}, ...]
```

All errors from the core pipeline surface as Python `RuntimeError`.
