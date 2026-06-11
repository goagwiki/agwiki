//! Renders [`agwiki_core::event::IngestEvent`]s into the exact terminal bytes the
//! `agwiki` CLI emits.
//!
//! Two output shapes, selected by whether `--progress` is active:
//! - **NDJSON mode** (default): each agent event is serialized as one JSON line on
//!   **stdout**; agent stderr is written verbatim to **stderr**.
//! - **Progress mode** (`--progress`): a live, deduplicated human-readable view of the
//!   agent stream is written to **stderr** (no per-event NDJSON on stdout), with a token
//!   footer flushed at the end of each agent run and `--- <key> ---` separators between
//!   files in folder mode.
//!
//! Skip notices (resume mode) are written to stderr as `SKIP: <key> (...)`.

use std::io::Write;

use agwiki_core::event::{IngestEvent, PlanAction};
use aikit_sdk::{AgentEvent, ProgressViewConfig, RunProgress};

/// Mirrors the previous in-core line-oriented progress sink: it pushes agent events
/// into a [`RunProgress`] view and writes each newly-formatted last row to stderr,
/// deduplicating repeats.
pub(crate) struct LineProgressSink {
    progress: RunProgress,
    last_row: Option<String>,
}

impl LineProgressSink {
    pub(crate) fn new() -> Self {
        Self {
            progress: RunProgress::new(ProgressViewConfig::default()),
            last_row: None,
        }
    }

    pub(crate) fn push(&mut self, agent_key: &str, event: &AgentEvent) {
        self.progress.push(agent_key, event);
        let new_last = self.progress.formatted_lines().last().map(str::to_string);
        if new_last != self.last_row {
            if let Some(ref row) = new_last {
                let _ = writeln!(std::io::stderr(), "{}", row);
            }
            self.last_row = new_last;
        }
    }

    pub(crate) fn reset(&mut self, next_source_key: &str) {
        self.emit_footer();
        self.progress.clear();
        self.last_row = None;
        eprintln!("--- {} ---", next_source_key);
    }

    pub(crate) fn emit_footer(&self) {
        if let Some(footer) = self.progress.token_footer() {
            let _ = writeln!(std::io::stderr(), "{}", footer);
        }
    }
}

/// Stateful renderer that turns the [`IngestEvent`] stream into terminal output.
///
/// Construct with [`IngestRenderer::new`], passing whether `--progress` is active, then
/// feed it as the `&mut dyn FnMut(IngestEvent)` sink to core ingest via [`Self::sink`].
pub struct IngestRenderer {
    progress: bool,
    /// Per-run progress view (recreated implicitly via push/footer cycles).
    run_sink: Option<LineProgressSink>,
    /// Folder-level sink used only for `--- key ---` separators and the final footer,
    /// mirroring the previously-unpushed folder progress sink (its footer is empty).
    folder_sink: Option<LineProgressSink>,
}

impl IngestRenderer {
    /// Create a renderer. `progress` selects progress (stderr) vs NDJSON (stdout) output.
    pub fn new(progress: bool) -> Self {
        Self {
            progress,
            run_sink: if progress {
                Some(LineProgressSink::new())
            } else {
                None
            },
            folder_sink: if progress {
                Some(LineProgressSink::new())
            } else {
                None
            },
        }
    }

    /// Render a single event to terminal output.
    pub fn handle(&mut self, event: IngestEvent) {
        match event {
            IngestEvent::Agent { agent_key, event } => {
                if self.progress {
                    if let Some(sink) = self.run_sink.as_mut() {
                        sink.push(&agent_key, &event);
                    }
                } else if let Ok(s) = serde_json::to_string(&event) {
                    println!("{}", s);
                }
            }
            IngestEvent::AgentRunFinished => {
                if let Some(sink) = self.run_sink.as_ref() {
                    sink.emit_footer();
                }
            }
            IngestEvent::AgentStderr(bytes) => {
                let _ = std::io::stderr().write_all(&bytes);
            }
            IngestEvent::ProgressReset { source_key } => {
                if let Some(sink) = self.folder_sink.as_mut() {
                    sink.reset(&source_key);
                }
            }
            IngestEvent::ProgressFinalFooter => {
                if let Some(sink) = self.folder_sink.as_ref() {
                    sink.emit_footer();
                }
            }
            IngestEvent::Skipped { source_key } => {
                eprintln!(
                    "SKIP: {} (already ingested under same policy/content/agent/model)",
                    source_key
                );
            }
            IngestEvent::Planned {
                source_key,
                action,
                reason,
                external_id,
            } => {
                let action_str = match action {
                    PlanAction::Ingest => "ingest",
                    PlanAction::Skip => "skip",
                };
                let line = serde_json::json!({
                    "source": source_key,
                    "action": action_str,
                    "reason": reason,
                    "external_id": external_id,
                });
                println!("{}", line);
            }
        }
    }

    /// Borrow a closure suitable to pass as core ingest's event sink.
    pub fn sink(&mut self) -> impl FnMut(IngestEvent) + Send + '_ {
        move |event| self.handle(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_sdk::{
        AgentEvent, AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole,
        StreamMessage,
    };

    fn make_assistant_event(text: &str) -> AgentEvent {
        AgentEvent {
            agent_key: "test-agent".to_string(),
            seq: 0,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::StreamMessage(StreamMessage {
                text: text.to_string(),
                phase: MessagePhase::Delta,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: AgentEventStream::Stdout,
                raw_line_seq: 0,
                turn_id: None,
            }),
        }
    }

    #[test]
    fn line_progress_sink_push_produces_row() {
        let mut sink = LineProgressSink::new();
        let event = make_assistant_event("hello world");
        sink.push("test-agent", &event);
        assert_eq!(sink.last_row.as_deref(), Some("assistant> hello world"));
    }

    #[test]
    fn line_progress_sink_deduplication() {
        let mut sink = LineProgressSink::new();
        let event1 = make_assistant_event("hello");
        let event2 = make_assistant_event("hello");
        sink.push("test-agent", &event1);
        let row_after_first = sink.last_row.clone();
        sink.push("test-agent", &event2);
        let row_after_second = sink.last_row.clone();
        assert_eq!(row_after_first, row_after_second);
        assert_eq!(row_after_first.as_deref(), Some("assistant> hello"));
    }

    #[test]
    fn line_progress_sink_ring_buffer_eviction() {
        let config = ProgressViewConfig {
            max_rows: 3,
            ..Default::default()
        };
        let mut sink = LineProgressSink {
            progress: RunProgress::new(config),
            last_row: None,
        };
        for i in 0..5u32 {
            let event = AgentEvent {
                agent_key: "agent".to_string(),
                seq: i as u64,
                stream: AgentEventStream::Stdout,
                payload: AgentEventPayload::StreamMessage(StreamMessage {
                    text: format!("msg {i}"),
                    phase: MessagePhase::Delta,
                    role: MessageRole::Assistant,
                    kind: MessageKind::Message,
                    source: AgentEventStream::Stdout,
                    raw_line_seq: 0,
                    turn_id: None,
                }),
            };
            sink.push("agent", &event);
        }
        assert_eq!(sink.last_row.as_deref(), Some("assistant> msg 4"));
    }

    #[test]
    fn line_progress_sink_reset_clears_state() {
        let mut sink = LineProgressSink::new();
        let event = make_assistant_event("before reset");
        sink.push("test-agent", &event);
        assert!(sink.last_row.is_some());
        sink.reset("next-file.md");
        assert!(sink.last_row.is_none());
        assert_eq!(sink.progress.formatted_lines().count(), 0);
    }
}
