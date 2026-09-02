//! Stage 1: clear first-run interstitials before the real team is built.
//!
//! Widened from "sign-in" after Phase 0. A login is not the only thing that
//! blocks a fresh CLI — both Claude Code and Gemini CLI show a *trust this
//! folder* prompt the first time they run in an unfamiliar directory, which is
//! the **normal** case for "create a repo, launch a team into it".
//!
//! Runs in the target project directory, so the prompt raised here is the same
//! one Stage 2 would otherwise hit, and one bare pane per CLI clears it for
//! every pane that CLI will later occupy.

use crate::herdr::types::{AgentStatus, ReadSource, SplitDirection};
use crate::herdr::{HerdrCli, HerdrError};
use crate::settings::Settings;
use std::path::{Path, PathBuf};

/// Something lifted out of the pane text that a human will need to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub kind: HintKind,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKind {
    /// A URL to open — device-flow authorise pages, mostly.
    Url,
    /// A short pairing code, e.g. `WDJB-MJHT`.
    DeviceCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstRunState {
    /// Still starting.
    Waiting,
    /// Sitting on something a human must answer.
    NeedsYou,
    /// Reached its prompt. First-run is done for this CLI in this project.
    Verified,
}

#[derive(Debug, Clone)]
pub struct FirstRunPane {
    pub cli: String,
    pub display_name: String,
    pub pane_id: String,
    pub state: FirstRunState,
    /// Recent pane output, mirrored so the UI can show it without the user
    /// switching to the terminal.
    pub screen: String,
    pub hints: Vec<Hint>,
}

#[derive(Debug, Clone)]
pub struct FirstRunSession {
    pub workspace_id: String,
    pub project: PathBuf,
    pub panes: Vec<FirstRunPane>,
}

impl FirstRunSession {
    pub fn all_verified(&self) -> bool {
        self.panes
            .iter()
            .all(|p| p.state == FirstRunState::Verified)
    }

    pub fn pending(&self) -> Vec<&FirstRunPane> {
        self.panes
            .iter()
            .filter(|p| p.state != FirstRunState::Verified)
            .collect()
    }

    /// Record every CLI that reached its prompt.
    ///
    /// Only verified CLIs are written, so an abandoned pass caches nothing and
    /// the next launch simply asks again.
    pub fn apply_to(&self, settings: &mut Settings) {
        for pane in &self.panes {
            if pane.state == FirstRunState::Verified {
                settings.mark_verified(&pane.cli, &self.project);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum FirstRunEvent {
    Started { workspace_id: String },
    PaneCreated { cli: String, pane_id: String },
    StateChanged { cli: String, state: FirstRunState },
    HintFound { cli: String, hint: Hint },
}

/// One CLI to put through first-run.
#[derive(Debug, Clone)]
pub struct FirstRunTarget {
    pub cli: String,
    pub display_name: String,
    /// Resolved absolute path, or the base name if unresolved.
    pub binary: String,
}

pub struct FirstRun<'a> {
    cli: &'a HerdrCli,
}

impl<'a> FirstRun<'a> {
    pub fn new(cli: &'a HerdrCli) -> Self {
        FirstRun { cli }
    }

    /// Create a throwaway workspace with one bare pane per CLI.
    ///
    /// Each pane runs the **bare binary with no flags**: permission flags are
    /// irrelevant to signing in or answering a trust prompt, and could suppress
    /// the very prompt we are trying to surface.
    pub fn start(
        &self,
        project: &Path,
        targets: &[FirstRunTarget],
        on_event: &mut dyn FnMut(FirstRunEvent),
    ) -> Result<FirstRunSession, HerdrError> {
        let created = self
            .cli
            .workspace_create(project, Some("herdup first-run"), false)?;
        let workspace_id = created.workspace.workspace_id.clone();
        on_event(FirstRunEvent::Started {
            workspace_id: workspace_id.clone(),
        });

        let mut panes = Vec::new();
        let mut previous = created.root_pane.pane_id.clone();

        for (index, target) in targets.iter().enumerate() {
            let pane_id = if index == 0 {
                created.root_pane.pane_id.clone()
            } else {
                let pane = self.cli.pane_split(
                    &previous,
                    SplitDirection::Down,
                    None,
                    Some(project),
                    false,
                )?;
                pane.pane_id
            };
            previous = pane_id.clone();

            self.cli.pane_rename(&pane_id, &target.display_name)?;
            self.cli.pane_run(&pane_id, &target.binary)?;

            on_event(FirstRunEvent::PaneCreated {
                cli: target.cli.clone(),
                pane_id: pane_id.clone(),
            });
            panes.push(FirstRunPane {
                cli: target.cli.clone(),
                display_name: target.display_name.clone(),
                pane_id,
                state: FirstRunState::Waiting,
                screen: String::new(),
                hints: Vec::new(),
            });
        }

        Ok(FirstRunSession {
            workspace_id,
            project: project.to_path_buf(),
            panes,
        })
    }

    /// One polling round. The caller owns the loop, the sleeps and the deadline,
    /// which keeps this testable without waiting on a clock.
    pub fn poll_once(
        &self,
        session: &mut FirstRunSession,
        on_event: &mut dyn FnMut(FirstRunEvent),
    ) {
        for pane in &mut session.panes {
            if pane.state == FirstRunState::Verified {
                continue;
            }

            let status = self
                .cli
                .pane_get(&pane.pane_id)
                .map(|p| p.agent_status)
                .unwrap_or(AgentStatus::Unknown);

            if let Ok(screen) = self.cli.pane_read(&pane.pane_id, ReadSource::Recent, 40) {
                pane.screen = screen;
            }

            let next = match status {
                s if s.is_settled() => FirstRunState::Verified,
                AgentStatus::Blocked => FirstRunState::NeedsYou,
                _ => FirstRunState::Waiting,
            };

            for hint in extract_hints(&pane.screen) {
                if !pane.hints.contains(&hint) {
                    on_event(FirstRunEvent::HintFound {
                        cli: pane.cli.clone(),
                        hint: hint.clone(),
                    });
                    pane.hints.push(hint);
                }
            }

            if next != pane.state {
                pane.state = next;
                on_event(FirstRunEvent::StateChanged {
                    cli: pane.cli.clone(),
                    state: next,
                });
            }
        }
    }

    /// Close the throwaway workspace.
    ///
    /// Best-effort per pane: one stubborn pane must not leave the whole setup
    /// workspace behind.
    pub fn teardown(&self, session: &FirstRunSession) -> Result<(), HerdrError> {
        for pane in &session.panes {
            let _ = self.cli.pane_close(&pane.pane_id);
        }
        self.cli.workspace_close(&session.workspace_id)
    }
}

/// Pull URLs and device codes out of pane text.
///
/// Hand-rolled rather than regex: the shapes are narrow, and this keeps the
/// dependency list short for something that only ever produces copy buttons.
pub fn extract_hints(screen: &str) -> Vec<Hint> {
    let mut hints: Vec<Hint> = Vec::new();

    for token in screen.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | '<' | '>' | '(' | ')' | ',' | ';' | '`' | '[' | ']'
            )
        });

        if cleaned.starts_with("http://") || cleaned.starts_with("https://") {
            push_unique(
                &mut hints,
                Hint {
                    kind: HintKind::Url,
                    // Trailing sentence punctuation is not part of the URL.
                    value: cleaned.trim_end_matches(['.', ':']).to_string(),
                },
            );
        } else if is_device_code(cleaned) {
            push_unique(
                &mut hints,
                Hint {
                    kind: HintKind::DeviceCode,
                    value: cleaned.to_string(),
                },
            );
        }
    }
    hints
}

fn push_unique(hints: &mut Vec<Hint>, hint: Hint) {
    if !hints.contains(&hint) {
        hints.push(hint);
    }
}

/// `WDJB-MJHT` and friends: two uppercase alphanumeric runs joined by a hyphen.
///
/// Deliberately narrow. A false positive here only adds a stray copy button,
/// but a pattern loose enough to match ordinary words would add one constantly.
fn is_device_code(token: &str) -> bool {
    let Some((left, right)) = token.split_once('-') else {
        return false;
    };
    let ok = |part: &str| {
        part.len() >= 4
            && part.len() <= 8
            && part
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    };
    ok(left) && ok(right) && !right.contains('-')
}
