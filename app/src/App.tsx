import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  api,
  type AddableRole,
  type Cli,
  type FirstRunPane,
  type LaunchOptions,
  type Outcome,
  type Plan,
  type PreflightReport,
  type Progress,
  type ProjectStatus,
  type Template,
  type Workspace,
} from "./api";

// Vocabulary rule for everything user-visible in this file: no pane, template,
// briefing, workspace, session or blocked. Teammate, team, instructions, folder,
// "needs you". The backend keeps its own names; the translation happens here.

type Step = "project" | "team" | "preflight" | "firstrun" | "launching" | "done";

const RECENTS_KEY = "herdup.recentProjects";
const MAX_RECENTS = 4;

/** Per-machine convenience only — never read back by the backend. */
function readRecents(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(RECENTS_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((x) => typeof x === "string").slice(0, MAX_RECENTS) : [];
  } catch {
    return [];
  }
}

function rememberProject(path: string) {
  try {
    const next = [path, ...readRecents().filter((p) => p !== path)].slice(0, MAX_RECENTS);
    localStorage.setItem(RECENTS_KEY, JSON.stringify(next));
  } catch {
    /* private mode, cleared storage — the list is a nicety, not state we need */
  }
}

const basename = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

/** The backend returns roles keyed by id, so they arrive alphabetically. Offer
 *  them in the order a team is actually built up instead. */
const ROLE_ORDER = ["lead", "coder", "tester", "builds", "research"];
const ordered = <T extends { id: string }>(roles: T[]): T[] =>
  [...roles].sort((a, b) => {
    const ia = ROLE_ORDER.indexOf(a.id);
    const ib = ROLE_ORDER.indexOf(b.id);
    return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib);
  });

/** Deterministic bar lengths per lane — random would shimmer on every render. */
const BARS = [
  [86, 62, 74, 40],
  [70, 90, 52, 66],
  [58, 78, 88, 46],
  [92, 48, 68, 80],
  [64, 84, 44, 72],
  [76, 56, 82, 60],
];

export default function App() {
  const [step, setStep] = useState<Step>("project");
  const [error, setError] = useState<string | null>(null);

  const [project, setProject] = useState("");
  const [recents, setRecents] = useState<string[]>(readRecents);
  const [status, setStatus] = useState<ProjectStatus | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [templates, setTemplates] = useState<Template[]>([]);
  const [addable, setAddable] = useState<AddableRole[]>([]);
  const [clis, setClis] = useState<Cli[]>([]);
  const [templateId, setTemplateId] = useState("squad");
  const [skip, setSkip] = useState<number[]>([]);
  const [overrides, setOverrides] = useState<[number, string][]>([]);
  const [extra, setExtra] = useState<string[]>([]);

  const [plan, setPlan] = useState<Plan | null>(null);
  const [report, setReport] = useState<PreflightReport | null>(null);
  const [firstRun, setFirstRun] = useState<FirstRunPane[]>([]);
  const [progress, setProgress] = useState<Progress[]>([]);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [busy, setBusy] = useState(false);

  const options: LaunchOptions = useMemo(
    () => ({ project, template: templateId, skip, overrides, extra }),
    [project, templateId, skip, overrides, extra],
  );

  useEffect(() => {
    api.listTemplates().then(setTemplates).catch((e) => setError(String(e)));
    api.listAddableRoles().then(setAddable).catch(() => setAddable([]));
    api.listClis().then(setClis).catch((e) => setError(String(e)));
    api.defaultProjectsRoot().then((root) => root && setProject((p) => p || root));
  }, []);

  const refreshWorkspaces = useCallback(() => {
    api.listWorkspaces().then(setWorkspaces).catch(() => setWorkspaces([]));
  }, []);

  // Several projects can run side by side, so the first screen doubles as the
  // place you see them. Poll while it is showing, otherwise a team that starts
  // needing you never says so.
  useEffect(() => {
    if (step !== "project") return;
    refreshWorkspaces();
    const t = window.setInterval(refreshWorkspaces, 5000);
    return () => window.clearInterval(t);
  }, [step, refreshWorkspaces]);

  // The one warning belongs next to the choice that caused it, so version
  // control is checked the moment a folder is picked — not three screens later.
  useEffect(() => {
    if (!project) {
      setStatus(null);
      return;
    }
    let live = true;
    api
      .projectStatus(project)
      .then((s) => live && setStatus(s))
      .catch(() => live && setStatus(null));
    return () => {
      live = false;
    };
  }, [project]);

  // Re-plan whenever the team shape changes, so the picture is never stale.
  useEffect(() => {
    if (!project || !templateId) return;
    api.previewPlan(options).then(setPlan).catch((e) => setError(String(e)));
  }, [options, project, templateId]);

  const guard = useCallback(async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const pickFolder = () =>
    guard(async () => {
      const chosen = await openDialog({ directory: true, title: "Choose a project" });
      if (typeof chosen === "string") setProject(chosen);
    });

  const toTeam = () => {
    rememberProject(project);
    setRecents(readRecents());
    setStep("team");
  };

  /// Back to the start for another project, leaving the running team alone.
  ///
  /// Each launch gets its own herdr workspace, so several projects genuinely do
  /// run side by side — this is only the UI catching up with that.
  const startAnother = () => {
    setPlan(null);
    setReport(null);
    setFirstRun([]);
    setProgress([]);
    setOutcome(null);
    setSkip([]);
    setOverrides([]);
    setExtra([]);
    setError(null);
    setStep("project");
  };

  const toPreflight = () =>
    guard(async () => {
      const r = await api.runPreflight(options);
      setReport(r);
      setStep("preflight");
    });

  const beginLaunch = () =>
    guard(async () => {
      setProgress([]);
      setStep("launching");
      const result = await api.launch(options);
      setOutcome(result);
      setStep("done");
    });

  const toFirstRunOrLaunch = () =>
    guard(async () => {
      if (report && report.needs_first_run.length > 0) {
        const panes = await api.startFirstRun(options);
        setFirstRun(panes);
        setStep("firstrun");
      } else {
        await beginLaunch();
      }
    });

  useEffect(() => {
    const unlisten = api.onProgress((p) => setProgress((all) => [...all, p]));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // Two dots for the two decisions; the later steps are consequences, not
  // choices, so they do not add ticks the user has to account for.
  const dot = step === "project" ? 1 : 2;

  return (
    <div className="app">
      <div className="topbar">
        <span className="mark">herdup</span>
        <span className="dots" aria-hidden>
          <i className={dot === 1 ? "on" : "done"} />
          <i className={dot === 2 ? "on" : ""} />
        </span>
      </div>

      {error && (
        <div className="errbox" role="alert" data-testid="error">
          <strong>Something went wrong</strong>
          {error}
          <div className="actions">
            <button className="btn" onClick={() => setError(null)}>
              Dismiss
            </button>
          </div>
        </div>
      )}

      <main data-testid={`step-${step}`}>
        {step === "project" && (
          <ProjectStep
            project={project}
            setProject={setProject}
            status={status}
            recents={recents}
            pickFolder={pickFolder}
            workspaces={workspaces}
            onAttach={(w) =>
              guard(async () => {
                await api.attachWorkspace(w.workspace_id, w.path);
              })
            }
            onNext={toTeam}
            busy={busy}
          />
        )}

        {step === "team" && (
          <TeamStep
            project={project}
            templates={templates}
            addable={addable}
            plan={plan}
            templateId={templateId}
            setTemplateId={(id) => {
              setTemplateId(id);
              setSkip([]);
              setOverrides([]);
              setExtra([]);
            }}
            extra={extra}
            setExtra={setExtra}
            setSkip={setSkip}
            onBack={() => setStep("project")}
            onNext={toPreflight}
            busy={busy}
          />
        )}

        {step === "preflight" && report && (
          <PreflightStep
            report={report}
            plan={plan}
            onBack={() => setStep("team")}
            onNext={toFirstRunOrLaunch}
            busy={busy}
            clis={clis}
            onSwitch={(origin, cli) =>
              setOverrides((o) => [...o.filter(([i]) => i !== origin), [origin, cli]])
            }
            onDrop={(origin) => setSkip((s) => (s.includes(origin) ? s : [...s, origin]))}
          />
        )}

        {step === "firstrun" && (
          <FirstRunStep
            panes={firstRun}
            setPanes={setFirstRun}
            project={project}
            onDone={() =>
              guard(async () => {
                await api.finishFirstRun();
                await beginLaunch();
              })
            }
            busy={busy}
          />
        )}

        {step === "launching" && (
          <LaunchingStep progress={progress} roles={(plan?.panes ?? []).map((p) => p.role)} />
        )}

        {step === "done" && outcome && (
          <DoneStep
            outcome={outcome}
            project={project}
            setOutcome={setOutcome}
            setError={setError}
            onAnother={startAnother}
          />
        )}
      </main>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 1 · which project
// ---------------------------------------------------------------------------

function ProjectStep(props: {
  project: string;
  setProject: (p: string) => void;
  status: ProjectStatus | null;
  recents: string[];
  pickFolder: () => void;
  workspaces: Workspace[];
  onAttach: (w: Workspace) => void;
  onNext: () => void;
  busy: boolean;
}) {
  const s = props.status;
  // Quiet unless risky: a folder under version control with a way back says
  // nothing at all. Only a genuinely un-undoable choice speaks up.
  const risky = s?.exists && !s.versioned;
  const dirty = s?.exists && s.versioned && s.uncommitted > 0;

  return (
    <section>
      <h1>Put a team of AIs on your code.</h1>
      <p className="lede">
        They read it, change it, and tell you what they did. You stay in charge.
      </p>

      <div className="label">Which project</div>
      <div className="list">
        {props.recents.map((p) => (
          <button
            key={p}
            type="button"
            className="pick"
            aria-pressed={p === props.project}
            onClick={() => props.setProject(p)}
          >
            <span className="mk" aria-hidden />
            <span className="nm">{basename(p)}</span>
            <span className="pt">{p}</span>
          </button>
        ))}
        <button
          type="button"
          className="pick browse"
          data-testid="browse"
          onClick={props.pickFolder}
          disabled={props.busy}
        >
          <span className="mk" aria-hidden />
          <span className="nm">Choose a folder…</span>
        </button>
      </div>

      {/* The path stays editable, but demoted: it is no longer the first thing
          a newcomer meets. */}
      <div className="row" style={{ marginTop: 10 }}>
        <input
          value={props.project}
          onChange={(e) => props.setProject(e.target.value)}
          placeholder="or type a path"
          spellCheck={false}
          aria-label="Project folder"
          data-testid="project-input"
        />
      </div>

      {risky && (
        <div className="warnbox" data-testid="warn-unversioned">
          <span className="ic" aria-hidden>
            ▲
          </span>
          <div>
            <strong>{s!.name} has no version history</strong>
            <p>
              If they change something you don&apos;t like, there&apos;s no way back. Setting up
              git first gives you an undo.
            </p>
          </div>
        </div>
      )}
      {dirty && (
        <div className="warnbox" data-testid="warn-uncommitted">
          <span className="ic" aria-hidden>
            ▲
          </span>
          <div>
            <strong className="num">
              {s!.uncommitted} unsaved change{s!.uncommitted === 1 ? "" : "s"} in {s!.name}
            </strong>
            <p>
              Their edits will mix into work you haven&apos;t committed
              {s!.branch ? ` on ${s!.branch}` : ""}, so undo gets messy. Commit or stash first if
              you want a clean way back.
            </p>
          </div>
        </div>
      )}

      <NewRepoPanel setProject={props.setProject} />

      {props.workspaces.length > 0 && (
        <>
          <div className="label" style={{ marginTop: 22 }}>
            Already running
          </div>
          <ul className="rows">
            {props.workspaces.map((w) => (
              <li key={w.workspace_id} data-testid={`workspace-${w.workspace_id}`}>
                <strong>{w.label}</strong>
                <span className="grow muted">
                  {w.pane_count} teammate{w.pane_count === 1 ? "" : "s"}
                  {w.path ? ` · ${w.path}` : ""}
                </span>
                {w.blocked ? (
                  <span className="state warn">needs you</span>
                ) : (
                  <span className="state">{w.agent_status}</span>
                )}
                <button
                  className="btn"
                  data-testid={`attach-${w.workspace_id}`}
                  onClick={() => props.onAttach(w)}
                >
                  Open
                </button>
                {w.path && (
                  <button
                    className="btn quiet"
                    data-testid={`use-folder-${w.workspace_id}`}
                    onClick={() => props.setProject(w.path!)}
                  >
                    Use this folder
                  </button>
                )}
              </li>
            ))}
          </ul>
        </>
      )}

      <div style={{ marginTop: 22 }}>
        <button
          className="go"
          data-testid="project-next"
          disabled={!props.project || props.busy}
          onClick={props.onNext}
        >
          Continue
          <span aria-hidden>→</span>
        </button>
      </div>
    </section>
  );
}

/// Create a GitHub repo and use it as the project.
///
/// Collapsed by default: this is the only thing herdup does that reaches outside
/// the machine, so it should not sit open inviting an accidental click.
function NewRepoPanel(props: { setProject: (p: string) => void }) {
  const [open, setOpen] = useState(false);
  const [owners, setOwners] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [owner, setOwner] = useState("");
  const [isPublic, setIsPublic] = useState(false);
  const [into, setInto] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [created, setCreated] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    api.ghOwners().then(setOwners).catch(() => setOwners([]));
    api.defaultProjectsRoot().then((r) => r && setInto((v) => v || r));
  }, [open]);

  if (!open) {
    return (
      <p className="muted" style={{ marginTop: 12 }}>
        Starting something new?{" "}
        <button className="btn quiet" data-testid="new-repo-open" onClick={() => setOpen(true)}>
          Create a GitHub repository
        </button>
      </p>
    );
  }

  const create = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const repo = await api.createRepo({
        name,
        owner: owner || null,
        public: isPublic,
        into,
        description: null,
      });
      setCreated(repo.path);
      props.setProject(repo.path);
    } catch (e) {
      setProblem(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel" data-testid="new-repo" style={{ marginTop: 12 }}>
      <header>
        <strong>New GitHub repository</strong>
        <button className="btn quiet" onClick={() => setOpen(false)}>
          Cancel
        </button>
      </header>

      <div className="row">
        <select
          value={owner}
          data-testid="new-repo-owner"
          onChange={(e) => setOwner(e.target.value)}
        >
          <option value="">(default account)</option>
          {owners.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
        <input
          value={name}
          data-testid="new-repo-name"
          onChange={(e) => setName(e.target.value)}
          placeholder="repository name"
          spellCheck={false}
        />
      </div>

      <div className="row" style={{ marginTop: 8 }}>
        <input
          value={into}
          data-testid="new-repo-into"
          onChange={(e) => setInto(e.target.value)}
          placeholder="clone into which folder"
          spellCheck={false}
        />
      </div>

      <label className="ack" style={{ marginTop: 10 }}>
        <input
          type="checkbox"
          data-testid="new-repo-public"
          checked={isPublic}
          onChange={(e) => setIsPublic(e.target.checked)}
        />
        <span>
          {isPublic
            ? "PUBLIC — this repository will be visible to anyone."
            : "Private (recommended). Tick to make it public instead."}
        </span>
      </label>

      {problem && (
        <div className="errbox" data-testid="new-repo-error">
          {problem}
        </div>
      )}
      {created && (
        <p className="muted" data-testid="new-repo-created">
          Created and cloned to <code>{created}</code>. It is now the selected project.
        </p>
      )}

      <div className="actions">
        <button
          className="btn solid"
          data-testid="new-repo-create"
          disabled={!name || !into || busy || !!created}
          onClick={create}
        >
          {busy ? "Creating…" : "Create and clone"}
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 2 · who's on the team
// ---------------------------------------------------------------------------

function TeamStep(props: {
  project: string;
  templates: Template[];
  addable: AddableRole[];
  plan: Plan | null;
  templateId: string;
  setTemplateId: (id: string) => void;
  extra: string[];
  setExtra: (f: (e: string[]) => string[]) => void;
  setSkip: (f: (s: number[]) => number[]) => void;
  onBack: () => void;
  onNext: () => void;
  busy: boolean;
}) {
  const seg = useRef<HTMLDivElement>(null);
  const panes = props.plan?.panes ?? [];
  // The backend hands these back keyed by id, so they arrive alphabetically.
  // A size picker has to read 1, 2, 4, 6.
  const presets = useMemo(
    () => [...props.templates].sort((a, b) => a.panes.length - b.panes.length),
    [props.templates],
  );

  // The thumb travels to the active preset rather than cross-fading.
  useLayoutEffect(() => {
    const el = seg.current;
    if (!el) return;
    const thumb = el.querySelector<HTMLElement>(".thumb");
    const active = el.querySelector<HTMLElement>('[aria-pressed="true"]');
    if (!thumb) return;
    if (!active) {
      thumb.style.opacity = "0";
      return;
    }
    thumb.style.opacity = "1";
    thumb.style.transform = `translateX(${active.offsetLeft - 3}px)`;
    thumb.style.width = `${active.offsetWidth}px`;
  }, [props.templateId, props.templates.length, props.extra.length]);

  /// Removing a teammate: a template pane goes into `skip` by its ORIGIN (the
  /// compacted index shifts and would drop the wrong one); an added pane is
  /// removed from `extra` by position among the added panes.
  const removeAt = (paneIndex: number) => {
    const pane = panes[paneIndex];
    if (!pane) return;
    if (pane.origin !== null) {
      props.setSkip((s) => (s.includes(pane.origin!) ? s : [...s, pane.origin!]));
      return;
    }
    const addedBefore = panes.slice(0, paneIndex).filter((p) => p.origin === null).length;
    props.setExtra((e) => e.filter((_, i) => i !== addedBefore));
  };

  const current = presets.find((t) => t.id === props.templateId);
  // A hand-edited line-up no longer matches its preset, so say so rather than
  // leaving a preset highlighted that no longer describes the team.
  const edited = props.extra.length > 0 || (current && panes.length !== current.panes.length);
  const hasLead = panes.some((p) => p.coordinator);

  return (
    <section>
      <button className="back" type="button" data-testid="back" onClick={props.onBack}>
        <span aria-hidden>←</span>
        <span className="nm">{basename(props.project)}</span>
        <span className="ch">Change</span>
      </button>

      {/* The picture of what you're about to get. */}
      <div className="label">Your workspace</div>
      <div className="stage">
        <div className="chrome" aria-hidden>
          <i />
          <i />
          <i />
          <span className="t">{basename(props.project)}</span>
        </div>
        {panes.length === 0 ? (
          <div className="stage-empty">Nobody on the team yet</div>
        ) : (
          <div className="lanes">
            {panes.map((p, i) => (
              <div
                className={`lane${p.coordinator ? " lead" : ""}`}
                key={`${p.role}-${i}`}
                title={`${p.role} — ${p.cli_display}`}
                data-testid={`lane-${i}`}
              >
                <span className="nm">{p.role}</span>
                {BARS[i % BARS.length].map((w, n) => (
                  <span className="bar" key={n} style={{ width: `${w}%` }} />
                ))}
                <button
                  type="button"
                  className="drop"
                  aria-label={`Remove ${p.role}`}
                  data-testid={`drop-${i}`}
                  onClick={() => removeAt(i)}
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="label">Who&apos;s on it</div>
      <div className="seg" ref={seg}>
        <span className="thumb" aria-hidden />
        {presets.map((t) => (
          <button
            key={t.id}
            type="button"
            className="segbtn"
            data-testid={`template-${t.id}`}
            aria-pressed={!edited && t.id === props.templateId}
            onClick={() => props.setTemplateId(t.id)}
          >
            <span className="n">{t.panes.length}</span>
            <span className="l">{t.display_name}</span>
          </button>
        ))}
      </div>
      <p className="pitch">
        {edited
          ? "Your own line-up — pick a size above to start over."
          : (current?.description ?? "")}
      </p>

      <div className="add">
        {ordered(props.addable).map((r) => (
          <button
            key={r.id}
            type="button"
            className="btn"
            title={r.summary}
            data-testid={`add-${r.id}`}
            // One lead per team; the backend rejects a second, so don't offer it.
            disabled={r.id === "lead" && hasLead}
            onClick={() => props.setExtra((e) => [...e, r.id])}
          >
            + {r.display_name}
          </button>
        ))}
      </div>

      {props.plan && props.plan.manual_briefings > 0 && (
        <p className="muted" style={{ marginBottom: 14 }}>
          <span className="num">{props.plan.manual_briefings}</span> of them use a tool herdup
          can&apos;t safely type into unattended, so you&apos;ll release those instructions yourself
          after a look.
        </p>
      )}

      <button
        className="go"
        data-testid="team-next"
        onClick={props.onNext}
        disabled={props.busy || panes.length === 0}
      >
        Continue
        <span aria-hidden>→</span>
      </button>
      <p className="note num">
        {panes.length} teammate{panes.length === 1 ? "" : "s"} · they can read and change files in
        that folder, nowhere else
      </p>
    </section>
  );
}

// ---------------------------------------------------------------------------
// 3 · the check. Not a designed screen — restyled and re-worded only.
// ---------------------------------------------------------------------------

function PreflightStep(props: {
  report: PreflightReport;
  plan: Plan | null;
  clis: Cli[];
  onBack: () => void;
  onNext: () => void;
  onSwitch: (origin: number, cli: string) => void;
  onDrop: (origin: number) => void;
  busy: boolean;
}) {
  const r = props.report;
  // Warnings are acknowledged individually. A launch puts agents with
  // file-editing permissions into this folder; that should never be one click
  // away from a mistyped path.
  const [ack, setAck] = useState<Set<number>>(new Set());
  const allAcked = r.warnings.every((_, i) => ack.has(i));
  const missing = r.clis.filter((c) => !c.installed);

  return (
    <section>
      <button className="back" type="button" data-testid="back" onClick={props.onBack}>
        <span aria-hidden>←</span>
        <span className="nm">Back to the team</span>
      </button>

      <h2>One last look</h2>
      <p className="lede num">
        About to start {props.plan?.panes.length ?? 0} teammate
        {(props.plan?.panes.length ?? 0) === 1 ? "" : "s"} in {r.project}
        {r.git_branch ? ` on ${r.git_branch}` : ""}.
      </p>

      {r.warnings.map((w, i) => (
        <label key={i} className="ack" data-testid={`warning-${i}`}>
          <input
            type="checkbox"
            data-testid={`ack-${i}`}
            checked={ack.has(i)}
            onChange={(e) =>
              setAck((s) => {
                const next = new Set(s);
                if (e.target.checked) next.add(i);
                else next.delete(i);
                return next;
              })
            }
          />
          <span>{w}</span>
        </label>
      ))}

      {r.blocking.length > 0 && (
        <div className="errbox" data-testid="blocking">
          <strong>Fix these before starting</strong>
          <ul>
            {r.blocking.map((b) => (
              <li key={b}>{b}</li>
            ))}
          </ul>
        </div>
      )}

      {/* Quiet unless risky: a healthy environment shows one line, not a table. */}
      {missing.length === 0 && r.herdr_ok ? (
        <p className="state ok" style={{ marginBottom: 14 }}>
          Everything they need is installed
        </p>
      ) : (
        <ul className="rows">
          {!r.herdr_ok && (
            <li>
              <strong>herdr</strong>
              <span className="grow muted">{r.herdr_note ?? r.herdr}</span>
              <span className="state warn">problem</span>
            </li>
          )}
          {missing.map((c) => (
            <li key={c.id}>
              <strong>{c.display_name}</strong>
              <span className="grow muted">
                not installed
                {c.install_command ? (
                  <>
                    {" · "}
                    <code>{c.install_command}</code>
                  </>
                ) : null}
              </span>
              {c.alternatives.length > 0 &&
                props.plan?.panes
                  .filter((p) => p.cli === c.id && p.origin !== null)
                  .map((p) => (
                    <span key={p.index} className="actions" style={{ marginTop: 0 }}>
                      <button
                        className="btn"
                        onClick={() => props.onSwitch(p.origin!, c.alternatives[0])}
                      >
                        {p.role}: use {c.alternatives[0]}
                      </button>
                      <button className="btn quiet" onClick={() => props.onDrop(p.origin!)}>
                        drop
                      </button>
                    </span>
                  ))}
            </li>
          ))}
        </ul>
      )}

      {r.platform_note && (
        <p className="muted" data-testid="platform-note">
          {r.platform_note}
        </p>
      )}

      <button
        className="go"
        data-testid="preflight-next"
        onClick={props.onNext}
        disabled={!r.can_launch || !allAcked || props.busy}
      >
        {r.needs_first_run.length > 0 ? "Set up access" : "Start working"}
        <span aria-hidden>→</span>
      </button>
      {!allAcked && <p className="note">Tick the boxes above to continue.</p>}
    </section>
  );
}

// ---------------------------------------------------------------------------
// 4 · first run, re-worded as "approve access"
// ---------------------------------------------------------------------------

function FirstRunStep(props: {
  panes: FirstRunPane[];
  setPanes: (p: FirstRunPane[]) => void;
  project: string;
  onDone: () => void;
  busy: boolean;
}) {
  const timer = useRef<number | null>(null);

  useEffect(() => {
    timer.current = window.setInterval(() => {
      api.pollFirstRun().then(props.setPanes).catch(() => {});
    }, 2000);
    return () => {
      if (timer.current) window.clearInterval(timer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const allDone = props.panes.length > 0 && props.panes.every((p) => p.state === "verified");

  return (
    <section>
      <h2>Approve access</h2>
      <p className="lede">
        Each tool asks once whether it may work in this folder. Answer in the terminal — this
        updates as you go.
      </p>
      <div className="actions" style={{ marginTop: 0, marginBottom: 16 }}>
        <button className="btn" onClick={() => api.openTerminal(props.project).catch(() => {})}>
          Open terminal
        </button>
      </div>

      {props.panes.map((p) => (
        <div key={p.cli} className="panel">
          <header>
            <strong>{p.display_name}</strong>
            <span
              className={`state ${p.state === "verified" ? "ok" : p.state === "needs_you" ? "warn" : "busy"}`}
            >
              {p.state === "verified"
                ? "ready"
                : p.state === "needs_you"
                  ? "needs you"
                  : "starting"}
            </span>
          </header>
          {p.hints.length > 0 && (
            <ul className="rows" style={{ marginBottom: 0 }}>
              {p.hints.map((h) => (
                <li key={h.value}>
                  <span className="tag">{h.kind}</span>
                  <code className="grow">{h.value}</code>
                  <button className="btn quiet" onClick={() => navigator.clipboard.writeText(h.value)}>
                    Copy
                  </button>
                </li>
              ))}
            </ul>
          )}
          {p.screen && <pre className="screen">{p.screen}</pre>}
        </div>
      ))}

      <button className="go" onClick={props.onDone} disabled={props.busy} style={{ marginTop: 16 }}>
        {allDone ? "Start working" : "Skip and start anyway"}
        <span aria-hidden>→</span>
      </button>
    </section>
  );
}

// ---------------------------------------------------------------------------
// 5 · launching
// ---------------------------------------------------------------------------

type Phase = "waiting" | "starting" | "ready" | "briefed" | "attention";

/// What each teammate is doing while it waits for work.
///
/// Keyed by role with any trailing number stripped, so "Coder 1" and "Coder 2"
/// share a line. Flavour only — the real state is the dot beside it.
const IDLE_QUIP: Record<string, string> = {
  pm: "deciding who does what",
  lead: "deciding who does what",
  dev: "cracking its knuckles",
  developer: "cracking its knuckles",
  coder: "cracking its knuckles",
  reviewer: "putting its reading glasses on",
  qa: "looking for something to break",
  tester: "looking for something to break",
  builds: "warming up the compiler",
  research: "opening far too many tabs",
};

const quipFor = (role: string) =>
  IDLE_QUIP[role.toLowerCase().replace(/\s*\d+$/, "")] ?? "waiting for orders";

// Never witty about a problem: anything that needs a human reads plainly.
const PHASE_TEXT: Record<Exclude<Phase, "ready" | "attention">, string> = {
  waiting: "not started yet",
  starting: "pulling up a chair",
  briefed: "on it",
};

function LaunchingStep(props: { progress: Progress[]; roles: string[] }) {
  const last = props.progress.filter((p) => p.kind === "step").at(-1);
  const pct = last?.total ? Math.round(((last.index ?? 0) / last.total) * 100) : 0;

  // Fold the event stream into one current state per teammate. The raw events
  // carry herdr's own words and pane ids; none of that reaches the screen.
  const state = new Map<string, { phase: Phase; note?: string }>();
  for (const p of props.progress) {
    if (!p.role) continue;
    const set = (phase: Phase, note?: string) => state.set(p.role!, { phase, note });
    if (p.kind === "pane_created") set("starting");
    else if (p.kind === "pane_ready") set("ready");
    else if (p.kind === "briefed") set("briefed");
    else if (p.kind === "needs_attention" || p.kind === "briefing_withheld")
      set("attention", p.detail ?? undefined);
  }

  const failure = props.progress.find((p) => p.kind === "failed");
  const done = props.roles.filter((r) => state.get(r)?.phase === "briefed").length;
  const headline =
    pct < 40 ? "Rounding up the team" : pct < 85 ? "Handing out the work" : "Almost there";

  return (
    <section>
      <h2>{failure ? "That didn't go to plan" : headline}</h2>
      <p className="lede num">
        {failure
          ? "Everyone already started was left running — they may hold real work."
          : `Twenty seconds, give or take. ${done} of ${props.roles.length} briefed.`}
      </p>

      <div className="bar-track">
        <div style={{ width: `${failure ? 100 : pct}%` }} />
      </div>

      <ul className="rows">
        {props.roles.map((role) => {
          const s = state.get(role) ?? { phase: "waiting" as Phase };
          const line =
            s.phase === "attention"
              ? (s.note ?? "needs you")
              : s.phase === "ready"
                ? quipFor(role)
                : PHASE_TEXT[s.phase];
          return (
            <li key={role}>
              <strong>{role}</strong>
              <span className="grow muted">{line}</span>
              <span
                className={`state ${
                  s.phase === "briefed"
                    ? "ok"
                    : s.phase === "attention"
                      ? "warn"
                      : s.phase === "waiting"
                        ? ""
                        : "busy"
                }`}
              >
                {s.phase === "briefed"
                  ? "working"
                  : s.phase === "attention"
                    ? "needs you"
                    : s.phase === "waiting"
                      ? "queued"
                      : "starting"}
              </span>
            </li>
          );
        })}
      </ul>

      {failure?.detail && <div className="errbox">{failure.detail}</div>}
    </section>
  );
}

// ---------------------------------------------------------------------------
// 6 · done
// ---------------------------------------------------------------------------

function DoneStep(props: {
  outcome: Outcome;
  project: string;
  setOutcome: (o: Outcome) => void;
  setError: (e: string | null) => void;
  onAnother: () => void;
}) {
  const o = props.outcome;
  const release = (index: number) =>
    api
      .sendBriefingNow(index)
      .then(props.setOutcome)
      .catch((e) => props.setError(String(e)));

  const needing = o.panes.filter((p) => p.state === "needs_attention" || p.has_pending_briefing);

  return (
    <section>
      <h2>
        {o.failure
          ? "Stopped partway"
          : needing.length === 0
            ? "Your team is working"
            : "Your team is up — some need you"}
      </h2>

      {o.failure && (
        <div className="errbox">
          <strong>{o.failed_step}</strong>
          {o.failure}
          <p style={{ margin: "8px 0 0" }}>
            The ones already started were left running on purpose — they may hold real work.
          </p>
        </div>
      )}

      <ul className="rows">
        {o.panes.map((p) => (
          <li key={p.index}>
            <strong>{p.role}</strong>
            <span className="grow muted">{p.cli_display}</span>
            {p.state === "briefed" ? (
              <span className="state ok">working</span>
            ) : p.state === "needs_attention" ? (
              <span className="state warn">{p.reason ?? "needs you"}</span>
            ) : (
              <span className="state">{p.state.replace(/_/g, " ")}</span>
            )}
            {p.has_pending_briefing && (
              <button className="btn solid" onClick={() => release(p.index)}>
                Send instructions
              </button>
            )}
            {p.screen && p.state === "needs_attention" && <pre className="screen">{p.screen}</pre>}
          </li>
        ))}
      </ul>

      <button
        className="go"
        onClick={() => api.openTerminal(props.project).catch((e) => props.setError(String(e)))}
      >
        Open the team
        <span aria-hidden>→</span>
      </button>

      {/* This team keeps running. Several projects can go at once. */}
      <div className="actions" style={{ justifyContent: "center" }}>
        <button className="btn" data-testid="start-another" onClick={props.onAnother}>
          Start another project
        </button>
      </div>
      <p className="note">
        They keep working while you do. Or run <code>herdr --session {o.session}</code> yourself.
      </p>
    </section>
  );
}
