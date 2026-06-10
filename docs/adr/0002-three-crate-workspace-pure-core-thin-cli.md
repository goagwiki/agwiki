# Three-crate workspace: pure core, thin CLI, Python binding

agwiki becomes a Cargo **workspace** of three crates so the business logic can be consumed
by other projects (including Python), not just the CLI:

- **`agwiki-core`** — all business logic (ingest pipeline, content model, materialize,
  ledger/identity). It is **pure** in the sense that matters: explicit parameters in, an
  `IngestEvent` stream out, ledger I/O. It **reads no config files, knows no hooks, and
  writes nothing to stdout**. It depends on **aikit-sdk directly** as *the* agent runner —
  there is deliberately **no `AgentRunner` trait**, because abstracting the runner is
  speculative flexibility we reject. It does **not** depend on cli-framework.
- **`agwiki` (CLI)** — the thin binary on **cli-framework** (latest, `project-config`
  feature). It owns everything core refuses: discovering `agwiki.toml` (committed schema)
  and `.agwiki/config.toml` (git-ignored operator settings + `[hooks]`) via
  cli-framework's `project_config`, applying precedence (flag > `AGWIKI_*` env > TOML >
  default), rendering `IngestEvent`s to NDJSON, and running shell **hooks** by subscribing
  to the event stream.
- **`agwiki-py`** — a PyO3 binding wrapping `agwiki-core`, delivering `IngestEvent`s to a
  Python callback. Mirrors the aikit-sdk / aikit-py pattern.

## Why an event sink instead of a runner trait

The reusability we want is "drive the pipeline from a CLI, a server, or Python," not "swap
the agent engine." That reusability comes entirely from the **`IngestEvent` callback sink**
(`FnMut(IngestEvent)`), so the abstraction budget is spent there, not on the runner. Today's
lib already keeps cli-framework out of the business logic, so this is largely a packaging
move plus relocating stdout/stderr rendering out of core.

## Consequences

- `IngestEvent` becomes **public API** of `agwiki-core`; adding variants is a semver event.
- Bumping cli-framework from `76a83e0` to current crosses a **Command API change**; the CLI
  crate needs updating during the split.
- Config and hooks are a **CLI-only** concern; `agwiki-core` and `agwiki-py` consumers react
  to events in their own code.
