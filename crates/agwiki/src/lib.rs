//! agwiki binary-crate library surface.
//!
//! The `agwiki` binary is a thin CLI over [`agwiki_core`]. This crate additionally
//! owns the axum-based HTTP browse server ([`serve`]), the ingest event renderer
//! ([`ingest_render`]) that turns [`agwiki_core::event::IngestEvent`]s back into the
//! exact stdout/stderr bytes the CLI emits, and the operator [`config`] layer
//! (`.agwiki/config.toml`) — a CLI-only concern `agwiki-core` never touches.

pub mod config;
pub mod hooks;
pub mod ingest_render;
pub mod serve;
