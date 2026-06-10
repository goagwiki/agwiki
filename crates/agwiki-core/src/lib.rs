//! agwiki-core: pure business logic for the agwiki agent-based wiki.
//!
//! This crate contains init, ingest, validation, compile, skill export, and markdown
//! rendering. It performs **no** terminal I/O of its own: ingest emits [`event::IngestEvent`]
//! values to a caller-supplied sink so the binary crate can render NDJSON/stderr output.
//!
//! Ingest loads `<wiki-root>/ingest.md` from the content repository (not from this crate)
//! and runs an agent via `aikit_sdk`; agent events are surfaced through the event sink.
//! Wiki root for ingest, check, and export defaults to the process current directory when
//! `-C` is omitted (resolved by the binary).

pub mod compile;
pub mod event;
pub mod export_skill;
pub mod ingest;
pub mod init;
pub mod markdown_html;
pub mod toolkit;
pub mod upkeep;
pub mod validate;
