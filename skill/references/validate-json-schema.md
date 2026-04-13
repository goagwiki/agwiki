# `agwiki validate --format json`

Stable shape (paths are strings; **`wiki_root`** is absolute when canonicalization succeeds):

```json
{
  "wiki_root": "/path/to/wiki",
  "problems": [
    { "kind": "broken_link", "message": "…" },
    { "kind": "orphan", "message": "wiki/relative/path.md" }
  ]
}
```

An empty **`problems`** array means the wiki passed validation.
