# Contributing

## Build from source

Requires a [Rust toolchain](https://rustup.rs/) (edition / MSRV as declared in `Cargo.toml`).

```bash
git clone https://github.com/goagwiki/agwiki.git
cd agwiki
cargo install --path .
```

Or build without installing:

```bash
cargo build --release
# binary: target/release/agwiki
```

## Ingest template in this repo

The file **`prompts/ingest.md`** here is the **reference template** for content wikis. Each wiki that uses `agwiki ingest` must ship **`ingest.md`** at the wiki root (same directory as `wiki/`). The CLI does not fall back to the agwiki source tree. **`agwiki init`** writes **`ingest.md`** from this file at compile time (`include_str!` in `src/init.rs`); keep them aligned when editing the template.

## Checks before pushing

Project norms are summarized in [AGENTS.md](AGENTS.md). In short:

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`

Or run the CI-aligned script:

```bash
./scripts/run-tests.sh
```

## Testing approach for aikit-sdk integration

The `agwiki ingest` command delegates to `aikit-sdk` which spawns external agent processes (e.g. `codex`, `opencode`). Tests must not require real LLM APIs or network access.

**PATH stub strategy**: Tests create a minimal shell script named after the target agent (e.g. `codex`) in a temporary directory, then prepend that directory to `PATH` before running the command under test. The stub reads stdin and exits 0, satisfying `is_runnable()` checks (which probe `PATH`) and any execution logic.

Example stub script written to `<tmpdir>/codex`:

```sh
#!/bin/sh
while IFS= read -r line; do :; done
exit 0
```

The stub directory is prepended to `PATH` before the test and restored afterward:

```rust
let original_path = std::env::var("PATH").unwrap_or_default();
std::env::set_var("PATH", format!("{}:{}", stub_dir.path().display(), original_path));
// ... run test ...
std::env::set_var("PATH", original_path);
```

Because `PATH` is a global process environment variable, tests that mutate it acquire a `static PATH_MUTEX: Mutex<()>` lock to prevent race conditions when running in parallel. Unix-specific ingest tests are gated with `#[cfg(unix)]`.

This approach requires no mocking of aikit-sdk Rust types and works with any agent name listed by `aikit_sdk::runnable_agents()`.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): description`, imperative mood, first line under 72 characters.
