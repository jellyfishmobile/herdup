import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Mirrors the DTOs in src-tauri/src/lib.rs. The backend owns every decision;
// these are render-ready values, not things the UI reasons about.

export type TemplatePane = {
  role: string;
  cli: string;
  flags: string;
  coordinator: boolean;
};

export type Template = {
  id: string;
  display_name: string;
  description: string;
  panes: TemplatePane[];
};

export type Cli = {
  id: string;
  display_name: string;
  binary: string;
  kind: string | null;
  flag_presets: string[];
  auto_briefable: boolean;
  install_command: string | null;
  docs_url: string | null;
};

export type Workspace = {
  workspace_id: string;
  label: string;
  pane_count: number;
  agent_status: string;
  path: string | null;
  blocked: boolean;
};

export type PlannedPane = {
  /// Compacted index — shifts whenever a pane is dropped. Display only.
  index: number;
  /// Template index, or null for a pane the user added. Everything that feeds
  /// back into `skip` or `overrides` must use this, never `index`.
  origin: number | null;
  role: string;
  cli: string;
  cli_display: string;
  command: string;
  agent_name: string | null;
  coordinator: boolean;
  auto_brief: boolean;
  dropped_flags: string | null;
};

export type Plan = {
  workspace_label: string;
  panes: PlannedPane[];
  steps: string[];
  distinct_clis: string[];
  manual_briefings: number;
};

export type CliStatus = {
  id: string;
  display_name: string;
  binary: string;
  resolved: string | null;
  installed: boolean;
  first_run_done: boolean;
  install_command: string | null;
  docs_url: string | null;
  alternatives: string[];
};

export type PreflightReport = {
  herdr: string;
  herdr_ok: boolean;
  herdr_note: string | null;
  gh_ready: boolean;
  gh_account: string | null;
  gh_blocker: string | null;
  clis: CliStatus[];
  needs_first_run: string[];
  blocking: string[];
  /// Not blockers — things to acknowledge before agents start editing.
  warnings: string[];
  /// A standing platform caveat, not a per-launch problem.
  platform_note: string | null;
  project: string;
  git_branch: string | null;
  can_launch: boolean;
};

export type LaunchedPane = {
  index: number;
  role: string;
  cli_display: string;
  pane_id: string | null;
  agent_name: string | null;
  state: "briefed" | "ready" | "starting" | "not_created" | "needs_attention";
  reason: string | null;
  screen: string | null;
  has_pending_briefing: boolean;
};

export type Outcome = {
  workspace_id: string | null;
  panes: LaunchedPane[];
  briefed: number;
  failure: string | null;
  failed_step: string | null;
  session: string;
};

export type Hint = { kind: string; value: string };

export type FirstRunPane = {
  cli: string;
  display_name: string;
  pane_id: string;
  state: "waiting" | "needs_you" | "verified";
  screen: string;
  hints: Hint[];
};

export type Progress = {
  kind: string;
  index: number | null;
  total: number | null;
  role: string | null;
  detail: string | null;
};

/// A role the user can add beyond the template. Ids only travel back — the
/// briefing text for each lives in the core crate, never here.
export type AddableRole = {
  id: string;
  display_name: string;
  summary: string;
  cli: string;
};

/// Cheap, read-only look at a folder, for the moment a project is chosen.
export type ProjectStatus = {
  exists: boolean;
  name: string;
  versioned: boolean;
  branch: string | null;
  uncommitted: number;
};

export type LaunchOptions = {
  project: string;
  template: string;
  skip: number[];
  overrides: [number, string][];
  /// Ids from `listAddableRoles`, in the order they were added.
  /// `"coder"` uses the role default tool; `"coder:agy"` overrides it.
  extra: string[];
};

export type CreatedRepo = { url: string | null; path: string };

export const api = {
  ghOwners: () => invoke<string[]>("gh_owners"),
  createRepo: (args: {
    name: string;
    owner: string | null;
    public: boolean;
    into: string;
    description: string | null;
  }) => invoke<CreatedRepo>("create_repo", args),
  listTemplates: () => invoke<Template[]>("list_templates"),
  listAddableRoles: () => invoke<AddableRole[]>("list_addable_roles"),
  projectStatus: (project: string) => invoke<ProjectStatus>("project_status", { project }),
  listClis: () => invoke<Cli[]>("list_clis"),
  listWorkspaces: () => invoke<Workspace[]>("list_workspaces"),
  previewPlan: (options: LaunchOptions) => invoke<Plan>("preview_plan", { options }),
  runPreflight: (options: LaunchOptions) =>
    invoke<PreflightReport>("run_preflight", { options }),
  startFirstRun: (options: LaunchOptions) =>
    invoke<FirstRunPane[]>("start_first_run", { options }),
  pollFirstRun: () => invoke<FirstRunPane[]>("poll_first_run"),
  finishFirstRun: () => invoke<void>("finish_first_run"),
  launch: (options: LaunchOptions) => invoke<Outcome>("launch", { options }),
  sendBriefingNow: (index: number) => invoke<Outcome>("send_briefing_now", { index }),
  openTerminal: (project: string) => invoke<string>("open_terminal", { project }),
  attachWorkspace: (workspaceId: string, path: string | null) =>
    invoke<string>("attach_workspace", { workspaceId, path }),
  defaultProjectsRoot: () => invoke<string | null>("default_projects_root"),
  onProgress: (fn: (p: Progress) => void) => listen<Progress>("launch-progress", (e) => fn(e.payload)),
};
