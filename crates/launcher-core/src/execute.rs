//! Walking a [`LaunchPlan`] against a real herdr.
//!
//! Deliberately dumb: every interesting decision was made during planning, so
//! this module resolves [`PaneRef`]s to real pane ids, calls herdr, and reports
//! what happened. The one judgement it does make is the safety-critical one —
//! whether a pane is genuinely ready to be typed into.

use crate::herdr::types::{AgentStatus, ReadSource, WaitOutcome};
use crate::herdr::{HerdrCli, HerdrError};
use crate::plan::{BriefingGate, LaunchPlan, PaneRef, Step};

/// Why a pane will not be briefed automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionReason {
    /// herdr reports the pane is waiting on a human — a login, a permission
    /// prompt, or a first-run trust prompt.
    Blocked,
    /// The pane never settled within the timeout.
    Timeout,
    /// The pane looks idle, but this CLI's blocked-detection is unverified, so
    /// "looks idle" is not good enough (spec §5.1).
    UnverifiedCli,
}

impl AttentionReason {
    pub fn explain(self) -> &'static str {
        match self {
            AttentionReason::Blocked => {
                "the pane is waiting on you — a login, a permission prompt, or a \
                 first-run 'trust this folder' prompt"
            }
            AttentionReason::Timeout => "the CLI did not reach its prompt in time",
            AttentionReason::UnverifiedCli => {
                "this CLI's blocked-detection has not been verified, so herdup will \
                 not type into it unattended"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneState {
    /// Created, but its CLI has not settled yet.
    Starting,
    /// Observed sitting at its prompt. A necessary condition for briefing.
    Ready,
    /// Settled at its prompt and briefed.
    Briefed,
    /// Needs a human before anything is typed into it.
    NeedsAttention(AttentionReason),
    /// The plan stopped before this pane was created.
    NotCreated,
}

#[derive(Debug, Clone)]
pub struct LaunchedPane {
    pub pane: PaneRef,
    pub role: String,
    pub cli_display: String,
    /// `None` until the pane exists.
    pub pane_id: Option<String>,
    pub state: PaneState,
    /// The briefing that was withheld, ready for a human to release.
    pub pending_briefing: Option<String>,
    /// Recent pane output captured when it needed attention, so the UI can show
    /// *why* without the user switching to the terminal.
    pub screen: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Failure {
    pub step_index: usize,
    pub description: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub workspace_id: Option<String>,
    pub panes: Vec<LaunchedPane>,
    /// `Some` if a step failed. Earlier panes are left standing (spec §11).
    pub failure: Option<Failure>,
    pub steps_run: usize,
    pub steps_total: usize,
}

impl Outcome {
    pub fn succeeded(&self) -> bool {
        self.failure.is_none()
    }

    /// Panes a human must deal with before they are briefed.
    pub fn needing_attention(&self) -> Vec<&LaunchedPane> {
        self.panes
            .iter()
            .filter(|p| matches!(p.state, PaneState::NeedsAttention(_)))
            .collect()
    }

    pub fn briefed(&self) -> usize {
        self.panes
            .iter()
            .filter(|p| p.state == PaneState::Briefed)
            .count()
    }
}

/// Progress, as it happens.
#[derive(Debug, Clone)]
pub enum Event {
    StepStarted {
        index: usize,
        total: usize,
        description: String,
    },
    PaneCreated {
        pane: PaneRef,
        role: String,
        pane_id: String,
    },
    PaneReady {
        pane: PaneRef,
        role: String,
    },
    PaneNeedsAttention {
        pane: PaneRef,
        role: String,
        reason: AttentionReason,
    },
    Briefed {
        pane: PaneRef,
        role: String,
    },
    BriefingWithheld {
        pane: PaneRef,
        role: String,
        reason: AttentionReason,
    },
    Failed {
        step_index: usize,
        message: String,
    },
    Finished,
}

pub struct Executor<'a> {
    cli: &'a HerdrCli,
}

impl<'a> Executor<'a> {
    pub fn new(cli: &'a HerdrCli) -> Self {
        Executor { cli }
    }

    /// Run the plan. Never rolls back.
    ///
    /// A partly-built team is still useful, and its panes may already hold
    /// running agents — tearing them down on failure could destroy work
    /// (spec §11). So a failure stops the walk and reports where it stopped.
    pub fn execute(&self, plan: &LaunchPlan, on_event: &mut dyn FnMut(Event)) -> Outcome {
        let total = plan.steps.len();
        let mut ids: Vec<Option<String>> = vec![None; plan.panes.len()];
        let mut panes: Vec<LaunchedPane> = plan
            .panes
            .iter()
            .map(|p| LaunchedPane {
                pane: p.pane,
                role: p.role.clone(),
                cli_display: p.cli_display.clone(),
                pane_id: None,
                state: PaneState::NotCreated,
                pending_briefing: None,
                screen: None,
            })
            .collect();
        let mut workspace_id = None;
        let mut failure = None;
        let mut steps_run = 0;

        let descriptions = describe_steps(plan);

        for (index, step) in plan.steps.iter().enumerate() {
            on_event(Event::StepStarted {
                index,
                total,
                description: descriptions[index].clone(),
            });

            let result = self.run_step(step, &mut ids, &mut panes, &mut workspace_id, on_event);

            match result {
                Ok(()) => steps_run += 1,
                Err(e) => {
                    let message = e.to_string();
                    on_event(Event::Failed {
                        step_index: index,
                        message: message.clone(),
                    });
                    failure = Some(Failure {
                        step_index: index,
                        description: descriptions[index].clone(),
                        message,
                    });
                    break;
                }
            }
        }

        on_event(Event::Finished);
        Outcome {
            workspace_id,
            panes,
            failure,
            steps_run,
            steps_total: total,
        }
    }

    /// Release a briefing that was withheld, after a human has dealt with the
    /// pane. This is what the UI's "Send briefing now" button calls.
    pub fn send_pending_briefing(&self, pane: &mut LaunchedPane) -> Result<(), HerdrError> {
        let (Some(id), Some(text)) = (pane.pane_id.clone(), pane.pending_briefing.clone()) else {
            return Ok(());
        };
        self.type_briefing(&id, &text)?;
        pane.pending_briefing = None;
        pane.state = PaneState::Briefed;
        Ok(())
    }

    fn run_step(
        &self,
        step: &Step,
        ids: &mut [Option<String>],
        panes: &mut [LaunchedPane],
        workspace_id: &mut Option<String>,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<(), HerdrError> {
        match step {
            Step::CreateWorkspace { cwd, label } => {
                let created = self.cli.workspace_create(cwd, Some(label), false)?;
                *workspace_id = Some(created.workspace.workspace_id);
                let id = created.root_pane.pane_id;
                ids[0] = Some(id.clone());
                panes[0].pane_id = Some(id.clone());
                panes[0].state = PaneState::Starting;
                on_event(Event::PaneCreated {
                    pane: PaneRef(0),
                    role: panes[0].role.clone(),
                    pane_id: id,
                });
            }

            Step::SplitPane {
                from,
                creates,
                direction,
                ratio,
                cwd,
            } => {
                let from_id = resolve(ids, *from)?;
                let pane = self
                    .cli
                    .pane_split(&from_id, *direction, *ratio, Some(cwd), false)?;
                ids[creates.0] = Some(pane.pane_id.clone());
                panes[creates.0].pane_id = Some(pane.pane_id.clone());
                panes[creates.0].state = PaneState::Starting;
                on_event(Event::PaneCreated {
                    pane: *creates,
                    role: panes[creates.0].role.clone(),
                    pane_id: pane.pane_id,
                });
            }

            Step::RenamePane { pane, label } => {
                let id = resolve(ids, *pane)?;
                self.cli.pane_rename(&id, label)?;
            }

            Step::RunCommand { pane, command } => {
                let id = resolve(ids, *pane)?;
                self.cli.pane_run(&id, command)?;
            }

            Step::AwaitIdle { pane, timeout_ms } => {
                let id = resolve(ids, *pane)?;
                let outcome = self
                    .cli
                    .wait_agent_status(&id, AgentStatus::Idle, *timeout_ms)?;

                // Trust the observed status over the wait's exit code: a wait can
                // land while the pane is really sitting on a prompt.
                let status = self
                    .cli
                    .pane_get(&id)
                    .map(|p| p.agent_status)
                    .unwrap_or(AgentStatus::Unknown);

                let settled = outcome == WaitOutcome::Reached && status.is_settled();
                if settled {
                    panes[pane.0].state = PaneState::Ready;
                    on_event(Event::PaneReady {
                        pane: *pane,
                        role: panes[pane.0].role.clone(),
                    });
                } else {
                    let reason = if status == AgentStatus::Blocked {
                        AttentionReason::Blocked
                    } else {
                        AttentionReason::Timeout
                    };
                    panes[pane.0].state = PaneState::NeedsAttention(reason);
                    panes[pane.0].screen = self.capture_screen(&id);
                    on_event(Event::PaneNeedsAttention {
                        pane: *pane,
                        role: panes[pane.0].role.clone(),
                        reason,
                    });
                }
            }

            Step::SendBriefing { pane, text, gate } => {
                let id = resolve(ids, *pane)?;
                let rendered = text.render(&|r: PaneRef| {
                    ids.get(r.0)
                        .and_then(|x| x.clone())
                        .unwrap_or_else(|| format!("<pane {}>", r.0))
                });

                // Two independent gates; either one withholds.
                //   1. the pane must have been *observed* ready — anything else,
                //      including a state we never confirmed, withholds
                //   2. the CLI's blocked-detection must be verified (spec §5.1)
                let withheld = match panes[pane.0].state {
                    PaneState::NeedsAttention(reason) => Some(reason),
                    PaneState::Ready if *gate == BriefingGate::RequiresHuman => {
                        Some(AttentionReason::UnverifiedCli)
                    }
                    PaneState::Ready => None,
                    // Never observed ready: withhold rather than assume.
                    _ => Some(AttentionReason::Timeout),
                };

                match withheld {
                    Some(reason) => {
                        panes[pane.0].state = PaneState::NeedsAttention(reason);
                        panes[pane.0].pending_briefing = Some(rendered);
                        if panes[pane.0].screen.is_none() {
                            panes[pane.0].screen = self.capture_screen(&id);
                        }
                        on_event(Event::BriefingWithheld {
                            pane: *pane,
                            role: panes[pane.0].role.clone(),
                            reason,
                        });
                    }
                    None => {
                        self.type_briefing(&id, &rendered)?;
                        panes[pane.0].state = PaneState::Briefed;
                        on_event(Event::Briefed {
                            pane: *pane,
                            role: panes[pane.0].role.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn type_briefing(&self, pane_id: &str, text: &str) -> Result<(), HerdrError> {
        self.cli.pane_send_text(pane_id, text)?;
        self.cli.pane_send_keys(pane_id, &["Enter"])
    }

    /// Best-effort: a failure to read the screen must not fail the launch.
    fn capture_screen(&self, pane_id: &str) -> Option<String> {
        self.cli.pane_read(pane_id, ReadSource::Recent, 40).ok()
    }
}

fn resolve(ids: &[Option<String>], pane: PaneRef) -> Result<String, HerdrError> {
    ids.get(pane.0).and_then(|x| x.clone()).ok_or_else(|| {
        // Only reachable if a plan referenced a pane before creating it, which
        // plan generation forbids and a test asserts.
        HerdrError::Api {
            code: "unresolved_pane".into(),
            message: format!("pane {pane} was referenced before it was created"),
        }
    })
}

fn describe_steps(plan: &LaunchPlan) -> Vec<String> {
    plan.describe()
        .lines()
        .map(|l| {
            l.split_once(". ")
                .map(|(_, rest)| rest.to_string())
                .unwrap_or_else(|| l.to_string())
        })
        .collect()
}
