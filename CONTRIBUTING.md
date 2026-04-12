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

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): description`, imperative mood, first line under 72 characters.
