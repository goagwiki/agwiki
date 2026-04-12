//! Unified wiki validation: broken links and orphan pages.

use anyhow::Result;
use serde::Serialize;
use std::path::Path;

use crate::upkeep::{check_links, list_orphans};

/// Run link and orphan checks; same as [`ValidationReport::run`].
pub fn validate_wiki(wiki_home: &Path) -> Result<ValidationReport> {
    ValidationReport::run(wiki_home)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    BrokenLink,
    Orphan,
}

#[derive(Debug, Serialize)]
pub struct Problem {
    pub kind: ProblemKind,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ValidationReport {
    pub wiki_root: String,
    pub problems: Vec<Problem>,
}

impl ValidationReport {
    /// Build a report from [`check_links`](crate::upkeep::check_links) and [`list_orphans`](crate::upkeep::list_orphans).
    pub fn run(wiki_home: &Path) -> Result<Self> {
        let root_display = wiki_home
            .canonicalize()
            .unwrap_or_else(|_| wiki_home.to_path_buf())
            .display()
            .to_string();

        let mut problems = Vec::new();

        for line in check_links(wiki_home)? {
            problems.push(Problem {
                kind: ProblemKind::BrokenLink,
                message: line,
            });
        }

        for p in list_orphans(wiki_home)? {
            let rel = p
                .strip_prefix(wiki_home)
                .unwrap_or(&p)
                .display()
                .to_string();
            problems.push(Problem {
                kind: ProblemKind::Orphan,
                message: rel,
            });
        }

        Ok(ValidationReport {
            wiki_root: root_display,
            problems,
        })
    }

    pub fn is_clean(&self) -> bool {
        self.problems.is_empty()
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn to_text(&self) -> String {
        if self.problems.is_empty() {
            return format!("OK: no problems under {}", self.wiki_root);
        }
        let mut broken = Vec::new();
        let mut orphans = Vec::new();
        for p in &self.problems {
            match p.kind {
                ProblemKind::BrokenLink => broken.push(p.message.as_str()),
                ProblemKind::Orphan => orphans.push(p.message.as_str()),
            }
        }
        let mut out = String::new();
        if !broken.is_empty() {
            out.push_str("Broken links:\n");
            for m in broken {
                out.push_str(m);
                out.push('\n');
            }
        }
        if !orphans.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str("Orphan pages:\n");
            for m in orphans {
                out.push_str(m);
                out.push('\n');
            }
        }
        out.push_str(&format!("\n{} problem(s) total.\n", self.problems.len()));
        out
    }
}
