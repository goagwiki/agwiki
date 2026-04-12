//! agwiki library: wiki init, upkeep, validation, skill export, and ingest (prompt expansion + `aikit run`).
//!
//! Ingest loads `<wiki-root>/ingest.md` from the content repository (not from this crate).
//! The binary runs `ingest::run_aikit` (`aikit run --events`, required `-a`, optional `-m` / `--stream`).
//! Wiki root for ingest, validate, and export-skill defaults to the process current directory when `-C` is omitted.

pub mod export_skill;
pub mod ingest;
pub mod init;
pub mod toolkit;
pub mod upkeep;
pub mod validate;
