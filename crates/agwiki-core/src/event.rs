//! Ingest event sink.
//!
//! Core ingest functions never write to stdout/stderr. Instead they emit
//! [`IngestEvent`] values to a caller-supplied sink (`&mut dyn FnMut(IngestEvent)`),
//! and the binary crate renders them to the exact bytes the `agwiki` CLI produces
//! today (NDJSON of agent events on stdout, skip notices / progress / summaries on
//! stderr).
//!
//! The event stream is a faithful trace of the output operations the CLI must
//! perform, so the renderer can reproduce byte-identical output. Variants map to:
//! - [`IngestEvent::Agent`] — one agent event from the SDK stream.
//! - [`IngestEvent::AgentStderr`] — captured agent stderr bytes (written verbatim).
//! - [`IngestEvent::AgentRunFinished`] — end of one agent run (progress footer flush).
//! - [`IngestEvent::ProgressReset`] — folder boundary between files in progress mode.
//! - [`IngestEvent::ProgressFinalFooter`] — final footer flush for a folder run.
//! - [`IngestEvent::Skipped`] — a source skipped due to a matching resume record.

use aikit_sdk::AgentEvent;

/// An event emitted by core ingest to a caller-supplied sink.
///
/// The binary crate renders these to terminal output; core does no printing.
#[derive(Debug, Clone)]
pub enum IngestEvent {
    /// A single agent event from the SDK stream, with the agent key it came from.
    Agent {
        /// Agent key the event was produced for.
        agent_key: String,
        /// The raw agent event.
        event: AgentEvent,
    },
    /// Raw agent stderr bytes, to be written verbatim to the process stderr.
    AgentStderr(Vec<u8>),
    /// Marks the end of a single agent run (one `run_aikit` invocation).
    ///
    /// In progress mode the renderer flushes that run's progress footer here.
    AgentRunFinished,
    /// Folder-mode boundary emitted before processing a subsequent file in
    /// progress mode (mirrors the previous `--- <source_key> ---` reset notice).
    ProgressReset {
        /// Source key (or display path) of the next file.
        source_key: String,
    },
    /// Final progress footer flush at the end of a folder run (progress mode).
    ProgressFinalFooter,
    /// A source was skipped because a matching prior success record exists.
    Skipped {
        /// Stable source identity key that was skipped.
        source_key: String,
    },
}

/// A sink that receives [`IngestEvent`]s from core ingest.
pub type IngestSink<'a> = dyn FnMut(IngestEvent) + 'a;
