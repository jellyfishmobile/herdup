//! Turning a template into an ordered, inspectable list of herdr operations.
//!
//! Plan generation is a **pure function**: no processes, no herdr, no clock. The
//! executor (Phase 4) is then a dumb walker over the list. That split is what
//! lets the interesting logic — ordering, layout remapping, briefing gates — be
//! tested with herdr not installed at all, and lets the UI show what is about to
//! happen before anything happens.

use crate::herdr::types::SplitDirection;
use crate::registry::{BriefingTrust, Registry};
use crate::template::{flatten, PaneSpec, Template};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("pane '{role}' names cli '{cli}', which is not in the registry")]
    UnknownCli { role: String, cli: String },

    #[error("every pane was skipped; there is nothing to launch")]
    NothingToLaunch,

    #[error("cannot skip pane {index}: the template only has {count} panes")]
    SkipOutOfRange { index: usize, count: usize },

    #[error("a team may have at most one coordinator, but this plan has {count}")]
    MultipleCoordinators { count: usize },
}

pub type Result<T> = std::result::Result<T, PlanError>;

/// A pane the plan will create, addressed by creation order.
///
/// Real herdr pane ids (`w1:p3`) do not exist until the plan runs, so steps
/// reference panes positionally and the executor resolves them as it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaneRef(pub usize);

impl std::fmt::Display for PaneRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Whether the executor may send this briefing without a human first looking at
/// the pane. Derived from the CLI's [`BriefingTrust`] (spec §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BriefingGate {
    /// CLI's blocked-detection is verified: send as soon as the pane is idle.
    Auto,
    /// Default. Never sent automatically, however idle the pane claims to be —
    /// Phase 0 caught a CLI reporting `idle` behind a blocking modal.
    RequiresHuman,
}

impl BriefingGate {
    pub fn is_auto(self) -> bool {
        self == BriefingGate::Auto
    }
}

impl From<BriefingTrust> for BriefingGate {
    fn from(t: BriefingTrust) -> Self {
        if t.may_auto_brief() {
            BriefingGate::Auto
        } else {
            BriefingGate::RequiresHuman
        }
    }
}

/// One teammate, as the coordinator will be told about them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    pub role: String,
    pub cli_display: String,
    pub pane: PaneRef,
    /// herdr agent name, when the teammate is managed as an agent.
    ///
    /// A better handle than a pane id: herdr resolves it to whichever pane the
    /// agent currently occupies, and agent commands accept it directly.
    pub agent_name: Option<String>,
}

/// Briefing content. The coordinator's cannot be fully rendered at plan time
/// because it names real pane ids, so it stays symbolic until execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BriefingText {
    Literal(String),
    Coordinator {
        preamble: String,
        roster: Vec<RosterEntry>,
    },
}

impl BriefingText {
    /// Render to the single line that will actually be typed.
    ///
    /// `resolve` maps a [`PaneRef`] to the real pane id discovered at run time.
    pub fn render(&self, resolve: &dyn Fn(PaneRef) -> String) -> String {
        match self {
            BriefingText::Literal(text) => text.clone(),
            BriefingText::Coordinator { preamble, roster } => {
                let mut out = preamble.clone();
                out.push_str(" Your team — ");
                let members: Vec<String> = roster
                    .iter()
                    .map(|r| match &r.agent_name {
                        Some(name) => format!(
                            "{} is agent '{}' running {} (pane {})",
                            r.role,
                            name,
                            r.cli_display,
                            resolve(r.pane)
                        ),
                        None => format!(
                            "{} in pane {} running {}",
                            r.role,
                            resolve(r.pane),
                            r.cli_display
                        ),
                    })
                    .collect();
                out.push_str(&members.join("; "));
                // Commands verified against herdr 0.8.2. Agent names are the
                // handle to prefer: herdr resolves a name to whichever pane the
                // agent currently occupies, so it survives layout changes that
                // a remembered pane id would not.
                out.push_str(
                    ". Drive them with the herdr CLI, addressing each teammate by its AGENT NAME: \
                     `herdr agent read <name> --source recent-unwrapped --lines 120` to see what \
                     one is doing, `herdr agent prompt <name> \"<task>\" --wait --timeout 600000` \
                     to give it work and wait for it to settle, and `herdr agent get <name>` to \
                     check its state. `herdr agent prompt` refuses to write to an agent sitting \
                     at an approval dialog and returns agent_blocked instead: when that happens, \
                     read the pane and ask the user rather than answering the dialog yourself. \
                     Re-read the roster with `herdr agent list` if a name stops resolving.",
                );
                flatten(&out)
            }
        }
    }
}

/// One operation against herdr.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    CreateWorkspace {
        cwd: PathBuf,
        label: String,
    },
    SplitPane {
        from: PaneRef,
        creates: PaneRef,
        direction: SplitDirection,
        ratio: Option<f32>,
        cwd: PathBuf,
    },
    RenamePane {
        pane: PaneRef,
        label: String,
    },
    /// Start an agent through herdr's agent API.
    ///
    /// This both launches and waits: `agent start` returns only once herdr sees
    /// the agent ready for input, so there is no separate readiness step and no
    /// window between "looks ready" and "typed into". If the agent blocks on a
    /// startup prompt herdr says so immediately.
    StartAgent {
        pane: PaneRef,
        /// Unique herdr agent name, derived from the role.
        name: String,
        /// herdr's agent kind.
        kind: String,
        /// Native agent arguments, passed after `--`.
        args: Vec<String>,
        timeout_ms: u64,
    },
    /// Fallback for CLIs herdr has no agent kind for.
    ///
    /// The string *is* shell-interpreted by the pane's shell — that is the point
    /// of it (spec §6.3). There is no readiness signal on this path, so such a
    /// pane is never auto-briefed.
    RunCommand {
        pane: PaneRef,
        command: String,
    },
    SendBriefing {
        pane: PaneRef,
        text: BriefingText,
        gate: BriefingGate,
    },
}

/// A pane in the finished plan, with everything the UI needs to describe it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedPane {
    pub pane: PaneRef,
    /// Index of this pane in the *template*, or `None` if the user added it.
    ///
    /// [`PaneRef`] is the compacted index and shifts whenever a pane is dropped,
    /// so anything that feeds back into `skip` or `cli_overrides` — both of
    /// which are keyed on template indices — must use this instead.
    pub origin: Option<usize>,
    pub role: String,
    pub cli: String,
    pub cli_display: String,
    /// Base name; preflight resolves it to an absolute path.
    pub binary: String,
    pub command: String,
    /// herdr agent kind, when this CLI has one.
    pub kind: Option<String>,
    /// Unique herdr agent name, when driven through the agent API.
    pub agent_name: Option<String>,
    pub coordinator: bool,
    pub gate: BriefingGate,
    /// Flags the template specified that were **discarded** because this pane's
    /// CLI was swapped and the new CLI is not known to accept them.
    ///
    /// Carrying `--permission-mode acceptEdits` from Claude Code over to Gemini
    /// would at best fail to start and at worst mean something else entirely.
    /// Recorded rather than silently dropped so the UI can say what happened.
    pub dropped_flags: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchPlan {
    pub project: PathBuf,
    pub workspace_label: String,
    pub panes: Vec<PlannedPane>,
    pub steps: Vec<Step>,
}

impl LaunchPlan {
    pub fn coordinator(&self) -> Option<&PlannedPane> {
        self.panes.iter().find(|p| p.coordinator)
    }

    /// Panes that will not be briefed automatically, so the UI can say up front
    /// how much manual work a launch will need.
    pub fn requires_human_briefing(&self) -> Vec<&PlannedPane> {
        self.panes.iter().filter(|p| !p.gate.is_auto()).collect()
    }

    /// Distinct CLI ids, in first-use order. Preflight and first-run work
    /// per-CLI, not per-pane.
    pub fn distinct_clis(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for p in &self.panes {
            if !seen.contains(&p.cli.as_str()) {
                seen.push(&p.cli);
            }
        }
        seen
    }

    /// A human-readable dry run, so "what is this about to do to my machine" is
    /// answerable before anything runs.
    pub fn describe(&self) -> String {
        let mut out = String::new();
        let name = |r: PaneRef| {
            self.panes
                .get(r.0)
                .map(|p| format!("{} {}", r, p.role))
                .unwrap_or_else(|| r.to_string())
        };
        for (i, step) in self.steps.iter().enumerate() {
            let line = match step {
                Step::CreateWorkspace { cwd, label } => {
                    format!("create workspace {label:?} in {}", cwd.display())
                }
                Step::SplitPane {
                    from,
                    creates,
                    direction,
                    ratio,
                    ..
                } => format!(
                    "split {} {}{} -> {}",
                    name(*from),
                    direction.as_str(),
                    ratio.map(|r| format!(" @{r}")).unwrap_or_default(),
                    name(*creates)
                ),
                Step::RenamePane { pane, label } => format!("rename {} to {label:?}", pane),
                Step::StartAgent {
                    pane,
                    name: agent,
                    kind,
                    args,
                    timeout_ms,
                } => format!(
                    "start agent '{agent}' (kind {kind}) in {}{} — waits up to {}s for readiness",
                    name(*pane),
                    if args.is_empty() {
                        String::new()
                    } else {
                        format!(" -- {}", args.join(" "))
                    },
                    timeout_ms / 1000
                ),
                Step::RunCommand { pane, command } => {
                    format!(
                        "run in {}: {} (no agent kind — never auto-briefed)",
                        name(*pane),
                        command
                    )
                }
                Step::SendBriefing { pane, gate, .. } => format!(
                    "brief {} [{}]",
                    name(*pane),
                    if gate.is_auto() {
                        "automatic"
                    } else {
                        "WAITS FOR YOU"
                    }
                ),
            };
            out.push_str(&format!("{:>3}. {line}\n", i + 1));
        }
        out
    }
}

/// What to launch, plus any preflight remediations the user chose.
#[derive(Debug, Clone)]
pub struct LaunchRequest<'a> {
    pub project: &'a Path,
    pub template: &'a Template,
    pub workspace_label: Option<String>,
    /// Template pane indices to drop, e.g. because their CLI is not installed.
    pub skip: BTreeSet<usize>,
    /// Template pane index -> replacement CLI id, e.g. swapping codex for claude.
    pub cli_overrides: BTreeMap<usize, String>,
    /// Panes added beyond the template, in the order the user added them.
    ///
    /// Each attaches to the root pane. These carry no original template index,
    /// so `cli_overrides` never applies to them — an added pane already names
    /// the CLI the user chose.
    pub extra: Vec<PaneSpec>,
    pub await_timeout_ms: u64,
}

impl<'a> LaunchRequest<'a> {
    pub fn new(project: &'a Path, template: &'a Template) -> Self {
        LaunchRequest {
            project,
            template,
            workspace_label: None,
            skip: BTreeSet::new(),
            cli_overrides: BTreeMap::new(),
            extra: Vec::new(),
            await_timeout_ms: 30_000,
        }
    }

    pub fn skip_pane(mut self, index: usize) -> Self {
        self.skip.insert(index);
        self
    }

    /// Add a pane beyond the template's own.
    pub fn add_pane(mut self, spec: PaneSpec) -> Self {
        self.extra.push(spec);
        self
    }

    pub fn override_cli(mut self, index: usize, cli: impl Into<String>) -> Self {
        self.cli_overrides.insert(index, cli.into());
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.workspace_label = Some(label.into());
        self
    }
}

/// Build the plan. Pure — same inputs, same output, every time.
pub fn plan(request: &LaunchRequest<'_>, registry: &Registry) -> Result<LaunchPlan> {
    let template = request.template;

    if let Some(&bad) = request.skip.iter().find(|&&i| i >= template.panes.len()) {
        return Err(PlanError::SkipOutOfRange {
            index: bad,
            count: template.panes.len(),
        });
    }

    let kept = compact(template, &request.extra, &request.skip)?;

    // At most one coordinator: its briefing is the only one that names the
    // finished roster, and two of them would each claim to run the team.
    let coordinators = kept.iter().filter(|k| k.spec.coordinator).count();
    if coordinators > 1 {
        return Err(PlanError::MultipleCoordinators {
            count: coordinators,
        });
    }

    // Resolve every CLI before emitting anything, so a bad reference fails the
    // whole plan rather than producing a half-usable one.
    let mut panes = Vec::with_capacity(kept.len());
    let mut used_names: Vec<String> = Vec::new();
    for (new_index, k) in kept.iter().enumerate() {
        let cli_id = k
            .original_index
            .and_then(|i| request.cli_overrides.get(&i).cloned())
            .unwrap_or_else(|| k.spec.cli.clone());
        let entry = registry.get(&cli_id).ok_or_else(|| PlanError::UnknownCli {
            role: k.spec.role.clone(),
            cli: cli_id.clone(),
        })?;

        // A template's flags are written for the template's CLI. When the user
        // swaps the CLI, keep them only if the new CLI is known to accept them;
        // otherwise drop them and say so. Same rule as the registry itself:
        // never use a flag nobody verified for that CLI.
        let swapped = cli_id != k.spec.cli;
        let flags_wanted = k.spec.flags.trim();
        let accepted = entry
            .flag_presets
            .iter()
            .any(|preset| preset.trim() == flags_wanted);
        let (flags, dropped_flags) = if swapped && !flags_wanted.is_empty() && !accepted {
            ("", Some(flags_wanted.to_string()))
        } else {
            (flags_wanted, None)
        };

        // Without a herdr agent kind there is no readiness signal and no
        // `agent_blocked` guard, so such a pane can never be auto-briefed
        // however trusted the CLI otherwise is.
        let gate = if entry.has_agent_kind() {
            BriefingGate::from(entry.briefing_trust)
        } else {
            BriefingGate::RequiresHuman
        };

        let agent_name = entry
            .has_agent_kind()
            .then(|| unique_agent_name(&k.spec.role, &used_names));
        if let Some(name) = &agent_name {
            used_names.push(name.clone());
        }

        panes.push(PlannedPane {
            pane: PaneRef(new_index),
            origin: k.original_index,
            role: k.spec.role.clone(),
            cli: cli_id,
            cli_display: entry.display_name.clone(),
            binary: entry.binary.clone(),
            command: crate::template::command_line(&entry.binary, flags),
            kind: entry.kind.clone(),
            agent_name,
            coordinator: k.spec.coordinator,
            gate,
            dropped_flags,
        });
    }

    let label = request
        .workspace_label
        .clone()
        .unwrap_or_else(|| project_label(request.project, &template.display_name));

    let mut steps = Vec::new();
    steps.push(Step::CreateWorkspace {
        cwd: request.project.to_path_buf(),
        label: label.clone(),
    });

    // Create, rename and run every pane in order. Pane 0 is the workspace's
    // root pane and is not split into existence.
    for (index, k) in kept.iter().enumerate() {
        let pane = PaneRef(index);
        if index > 0 {
            let split = k
                .split
                .expect("compact() gives every non-root pane a split");
            steps.push(Step::SplitPane {
                from: PaneRef(split.from),
                creates: pane,
                direction: split.direction,
                ratio: split.ratio,
                cwd: request.project.to_path_buf(),
            });
        }
        steps.push(Step::RenamePane {
            pane,
            label: panes[index].role.clone(),
        });
        match (&panes[index].kind, &panes[index].agent_name) {
            (Some(kind), Some(name)) => steps.push(Step::StartAgent {
                pane,
                name: name.clone(),
                kind: kind.clone(),
                args: split_args(&k.spec.flags),
                timeout_ms: request.await_timeout_ms,
            }),
            _ => steps.push(Step::RunCommand {
                pane,
                command: panes[index].command.clone(),
            }),
        }
    }

    // Brief everyone except the coordinator...
    let coordinator = panes.iter().position(|p| p.coordinator);
    for (index, k) in kept.iter().enumerate() {
        if Some(index) == coordinator {
            continue;
        }
        steps.push(Step::SendBriefing {
            pane: PaneRef(index),
            text: BriefingText::Literal(k.spec.flattened_briefing()),
            gate: panes[index].gate,
        });
    }

    // ...then the coordinator last, because its briefing names the finished team.
    if let Some(index) = coordinator {
        let roster = panes
            .iter()
            .filter(|p| !p.coordinator)
            .map(|p| RosterEntry {
                role: p.role.clone(),
                cli_display: p.cli_display.clone(),
                pane: p.pane,
                agent_name: p.agent_name.clone(),
            })
            .collect();
        steps.push(Step::SendBriefing {
            pane: PaneRef(index),
            text: BriefingText::Coordinator {
                preamble: kept[index].spec.flattened_briefing(),
                roster,
            },
            gate: panes[index].gate,
        });
    }

    Ok(LaunchPlan {
        project: request.project.to_path_buf(),
        workspace_label: label,
        panes,
        steps,
    })
}

/// A kept pane, with its split remapped onto the compacted index space.
struct Kept<'a> {
    /// `None` for a pane the user added, which has no template index and so is
    /// never touched by `cli_overrides`.
    original_index: Option<usize>,
    spec: &'a PaneSpec,
    /// `None` only for the new pane 0.
    split: Option<ResolvedSplit>,
}

#[derive(Clone, Copy)]
struct ResolvedSplit {
    from: usize,
    direction: SplitDirection,
    ratio: Option<f32>,
}

/// Drop skipped panes, re-point any split that referenced them, then append the
/// user's added panes.
///
/// Dropping a pane orphans its children, so each survivor is re-attached to its
/// nearest surviving ancestor. If the original root is dropped, the first
/// survivor becomes the new root and loses its split. Added panes always hang
/// off the root, because the user placed no layout intent on them.
fn compact<'a>(
    template: &'a Template,
    extra: &'a [PaneSpec],
    skip: &BTreeSet<usize>,
) -> Result<Vec<Kept<'a>>> {
    let parent: Vec<Option<usize>> = template
        .panes
        .iter()
        .map(|p| p.split.map(|s| s.from))
        .collect();

    let survivors: Vec<usize> = (0..template.panes.len())
        .filter(|i| !skip.contains(i))
        .collect();
    if survivors.is_empty() && extra.is_empty() {
        return Err(PlanError::NothingToLaunch);
    }

    let new_index: BTreeMap<usize, usize> = survivors
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();

    let mut out = Vec::with_capacity(survivors.len());
    for (new, &old) in survivors.iter().enumerate() {
        let spec = &template.panes[old];
        let split = if new == 0 {
            None // whoever ends up first is the workspace root pane
        } else {
            // Walk up until we find an ancestor that survived.
            let mut ancestor = parent[old];
            while let Some(a) = ancestor {
                if new_index.contains_key(&a) {
                    break;
                }
                ancestor = parent[a];
            }
            let from = ancestor
                .and_then(|a| new_index.get(&a).copied())
                .unwrap_or(0);
            let original = spec.split;
            Some(ResolvedSplit {
                from,
                direction: original
                    .map(|s| s.direction)
                    .unwrap_or(SplitDirection::Right),
                ratio: original.and_then(|s| s.ratio),
            })
        };
        out.push(Kept {
            original_index: Some(old),
            spec,
            split,
        });
    }

    // Added panes come last and hang off the root — except when every template
    // pane was dropped, in which case the first added pane becomes the root.
    for spec in extra {
        let split = (!out.is_empty()).then_some(ResolvedSplit {
            from: 0,
            direction: SplitDirection::Right,
            ratio: None,
        });
        out.push(Kept {
            original_index: None,
            spec,
            split,
        });
    }
    Ok(out)
}

/// Split a flag string into argv elements for `agent start ... -- <args>`.
///
/// Whitespace splitting is sufficient because registry flag presets are only
/// ever shipped where verified, and none of the verified ones contain quoted
/// arguments. A user who needs quoting can edit the command their template
/// produces instead.
fn split_args(flags: &str) -> Vec<String> {
    flags.split_whitespace().map(str::to_string).collect()
}

/// A herdr agent name derived from a role.
///
/// herdr requires `[a-z][a-z0-9_-]{0,31}` and uniqueness among live agents, so
/// "Coder 1" becomes `coder-1`.
fn agent_name_for(role: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in role.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !out.is_empty() && !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    // Must start with a letter.
    let out = match out.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => out,
        _ => format!("a{}{}", if out.is_empty() { "" } else { "-" }, out),
    };
    out.chars().take(32).collect()
}

/// `agent_name_for`, with a numeric suffix if that name is already taken.
fn unique_agent_name(role: &str, used: &[String]) -> String {
    let base = agent_name_for(role);
    if !used.contains(&base) {
        return base;
    }
    for n in 2..1000 {
        let truncated: String = base.chars().take(28).collect();
        let candidate = format!("{truncated}-{n}");
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    base
}

/// `<folder> — <template>`, e.g. `herdup — Squad`.
fn project_label(project: &Path, template_name: &str) -> String {
    let folder = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| project.display().to_string());
    format!("{folder} — {template_name}")
}
