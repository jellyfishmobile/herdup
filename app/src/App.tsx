import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  api,
  type Cli,
  type FirstRunPane,
  type LaunchOptions,
  type Outcome,
  type Plan,
  type PreflightReport,
  type Progress,
  type Template,
  type Workspace,
} from "./api";

type Step = "project" | "team" | "preflight" | "firstrun" | "launching" | "done";

const STEPS: { id: Step; label: string }[] = [
  { id: "project", label: "Project" },
  { id: "team", label: "Team" },
  { id: "preflight", label: "Check" },
  { id: "firstrun", label: "First run" },
  { id: "launching", label: "Launch" },
  { id: "done", label: "Done" },
];

export default function App() {
  const [step, setStep] = useState<Step>("project");
  const [error, setError] = useState<string | null>(null);

  const [project, setProject] = useState("");
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [templates, setTemplates] = useState<Template[]>([]);
  const [clis, setClis] = useState<Cli[]>([]);
  const [templateId, setTemplateId] = useState("squad");
  const [skip, setSkip] = useState<number[]>([]);
  const [overrides, setOverrides] = useState<[number, string][]>([]);

  const [plan, setPlan] = useState<Plan | null>(null);
  const [report, setReport] = useState<PreflightReport | null>(null);
  const [firstRun, setFirstRun] = useState<FirstRunPane[]>([]);
  const [progress, setProgress] = useState<Progress[]>([]);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [busy, setBusy] = useState(false);

  const options: LaunchOptions = useMemo(
    () => ({ project, template: templateId, skip, overrides }),
    [project, templateId, skip, overrides],
  );

  useEffect(() => {
    api.listTemplates().then(setTemplates).catch((e) => setError(String(e)));
    api.listClis().then(setClis).catch((e) => setError(String(e)));
    api.listWorkspaces().then(setWorkspaces).catch(() => setWorkspaces([]));
    api.defaultProjectsRoot().then((root) => root && setProject((p) => p || root));
  }, []);

  // Re-plan whenever the team shape changes, so the preview is never stale.
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

  // Progress events stream in while the launch runs on a worker thread.
  useEffect(() => {
    const unlisten = api.onProgress((p) => setProgress((all) => [...all, p]));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const stepIndex = STEPS.findIndex((s) => s.id === step);

  return (
    <div className="app">
      <header>
        <h1>herdup</h1>
        <ol className="steps">
          {STEPS.map((s, i) => (
            <li key={s.id} className={i === stepIndex ? "on" : i < stepIndex ? "past" : ""}>
              {s.label}
            </li>
          ))}
        </ol>
      </header>

      {error && (
        <div className="error" role="alert">
          {error}
          <button onClick={() => setError(null)}>dismiss</button>
        </div>
      )}

      <main>
        {step === "project" && (
          <ProjectStep
            project={project}
            setProject={setProject}
            pickFolder={pickFolder}
            workspaces={workspaces}
            onNext={() => setStep("team")}
            busy={busy}
          />
        )}

        {step === "team" && (
          <TeamStep
            templates={templates}
            clis={clis}
            templateId={templateId}
            setTemplateId={(id) => {
              setTemplateId(id);
              setSkip([]);
              setOverrides([]);
            }}
            plan={plan}
            skip={skip}
            setSkip={setSkip}
            overrides={overrides}
            setOverrides={setOverrides}
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
            onSwitch={(index, cli) => setOverrides((o) => [...o.filter(([i]) => i !== index), [index, cli]])}
            onDrop={(index) => setSkip((s) => [...s, index])}
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

        {step === "launching" && <LaunchingStep progress={progress} />}

        {step === "done" && outcome && (
          <DoneStep
            outcome={outcome}
            project={project}
            setOutcome={setOutcome}
            setError={setError}
          />
        )}
      </main>
    </div>
  );
}

// ---------------------------------------------------------------------------

function ProjectStep(props: {
  project: string;
  setProject: (p: string) => void;
  pickFolder: () => void;
  workspaces: Workspace[];
  onNext: () => void;
  busy: boolean;
}) {
  return (
    <section>
      <h2>Which project?</h2>
      <div className="row">
        <input
          value={props.project}
          onChange={(e) => props.setProject(e.target.value)}
          placeholder="path to a project folder"
          spellCheck={false}
        />
        <button onClick={props.pickFolder} disabled={props.busy}>
          Browse…
        </button>
      </div>

      <h3>Already running in herdup's session</h3>
      {props.workspaces.length === 0 ? (
        <p className="muted">
          Nothing running yet. herdup uses its own herdr session, so it never touches
          workspaces you started yourself.
        </p>
      ) : (
        <ul className="list">
          {props.workspaces.map((w) => (
            <li key={w.workspace_id}>
              <strong>{w.label}</strong>
              <span className="muted">
                {w.pane_count} pane(s) · {w.agent_status}
              </span>
            </li>
          ))}
        </ul>
      )}

      <div className="actions">
        <button className="primary" disabled={!props.project || props.busy} onClick={props.onNext}>
          Next
        </button>
      </div>
    </section>
  );
}

function TeamStep(props: {
  templates: Template[];
  clis: Cli[];
  templateId: string;
  setTemplateId: (id: string) => void;
  plan: Plan | null;
  skip: number[];
  setSkip: (f: (s: number[]) => number[]) => void;
  overrides: [number, string][];
  setOverrides: (f: (o: [number, string][]) => [number, string][]) => void;
  onBack: () => void;
  onNext: () => void;
  busy: boolean;
}) {
  const setCli = (index: number, cli: string) =>
    props.setOverrides((o) => [...o.filter(([i]) => i !== index), [index, cli]]);

  return (
    <section>
      <h2>Which team?</h2>
      <div className="cards">
        {props.templates.map((t) => (
          <button
            key={t.id}
            className={`card ${t.id === props.templateId ? "on" : ""}`}
            onClick={() => props.setTemplateId(t.id)}
          >
            <strong>{t.display_name}</strong>
            <span>{t.panes.length} pane(s)</span>
            <span className="muted">{t.description}</span>
          </button>
        ))}
      </div>

      {props.plan && (
        <>
          <h3>Roles</h3>
          <table className="grid">
            <thead>
              <tr>
                <th>Role</th>
                <th>CLI</th>
                <th>Command</th>
                <th>Briefing</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {props.plan.panes.map((p) => (
                <tr key={p.index}>
                  <td>
                    {p.role}
                    {p.coordinator && <span className="tag">coordinator</span>}
                  </td>
                  <td>
                    <select value={p.cli} onChange={(e) => setCli(p.index, e.target.value)}>
                      {props.clis.map((c) => (
                        <option key={c.id} value={c.id}>
                          {c.display_name}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <code>{p.command}</code>
                    {p.dropped_flags && (
                      <div className="warn">
                        dropped <code>{p.dropped_flags}</code> — {p.cli_display} is not known to
                        accept it
                      </div>
                    )}
                  </td>
                  <td>
                    {p.auto_brief ? (
                      <span className="ok">automatic</span>
                    ) : (
                      <span className="warn">waits for you</span>
                    )}
                  </td>
                  <td>
                    <button onClick={() => props.setSkip((s) => [...s, p.index])}>drop</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          {props.plan.manual_briefings > 0 && (
            <p className="note">
              {props.plan.manual_briefings} pane(s) will not be briefed automatically. Those CLIs
              have unverified blocked-detection, so herdup will not type into them unattended —
              you release each briefing after looking at the pane.
            </p>
          )}
          {props.skip.length > 0 && (
            <p className="note">
              {props.skip.length} pane(s) dropped.{" "}
              <button onClick={() => props.setSkip(() => [])}>undo</button>
            </p>
          )}
        </>
      )}

      <div className="actions">
        <button onClick={props.onBack}>Back</button>
        <button className="primary" onClick={props.onNext} disabled={props.busy}>
          Check environment
        </button>
      </div>
    </section>
  );
}

function PreflightStep(props: {
  report: PreflightReport;
  plan: Plan | null;
  onBack: () => void;
  onNext: () => void;
  onSwitch: (index: number, cli: string) => void;
  onDrop: (index: number) => void;
  busy: boolean;
}) {
  const r = props.report;
  // Warnings must be acknowledged individually. A launch puts agents with
  // file-editing permissions into this folder; that should never be one click
  // away from a mistyped path.
  const [ack, setAck] = useState<Set<number>>(new Set());
  const allAcked = r.warnings.every((_, i) => ack.has(i));

  return (
    <section>
      <h2>Environment</h2>

      <div className="confirm">
        <div>
          Launching <strong>{props.plan?.panes.length ?? 0} agent(s)</strong> into
        </div>
        <code>{r.project}</code>
        {r.git_branch && <div className="muted">on branch {r.git_branch}</div>}
      </div>

      {r.warnings.map((w, i) => (
        <label key={i} className="ackbox">
          <input
            type="checkbox"
            checked={ack.has(i)}
            onChange={(e) =>
              setAck((s) => {
                const next = new Set(s);
                e.target.checked ? next.add(i) : next.delete(i);
                return next;
              })
            }
          />
          <span>{w}</span>
        </label>
      ))}
      <ul className="list">
        <li>
          <strong>{r.herdr}</strong>
          <span className={r.herdr_ok ? "ok" : "warn"}>{r.herdr_ok ? "ok" : "problem"}</span>
        </li>
        {r.herdr_note && <li className="muted">{r.herdr_note}</li>}
        <li>
          <strong>GitHub CLI</strong>
          <span className={r.gh_ready ? "ok" : "muted"}>
            {r.gh_ready ? `ready${r.gh_account ? ` (${r.gh_account})` : ""}` : r.gh_blocker}
          </span>
        </li>
      </ul>

      <h3>Agent CLIs</h3>
      <ul className="list">
        {r.clis.map((c) => (
          <li key={c.id}>
            <strong>{c.display_name}</strong>
            {c.installed ? (
              <span className="ok">
                found <code>{c.resolved}</code>
                {c.first_run_done ? " · first run done" : " · first run needed"}
              </span>
            ) : (
              <span className="warn">
                not found (looked for <code>{c.binary}</code>)
                {c.install_command && (
                  <div>
                    install: <code>{c.install_command}</code>
                  </div>
                )}
                {c.alternatives.length > 0 && props.plan && (
                  <div className="fixes">
                    {props.plan.panes
                      .filter((p) => p.cli === c.id)
                      .map((p) => (
                        <span key={p.index}>
                          {p.role}:
                          <button onClick={() => props.onSwitch(p.index, c.alternatives[0])}>
                            switch to {c.alternatives[0]}
                          </button>
                          <button onClick={() => props.onDrop(p.index)}>drop pane</button>
                        </span>
                      ))}
                  </div>
                )}
              </span>
            )}
          </li>
        ))}
      </ul>

      {r.blocking.length > 0 && (
        <div className="error">
          <strong>Resolve before launching:</strong>
          <ul>
            {r.blocking.map((b) => (
              <li key={b}>{b}</li>
            ))}
          </ul>
        </div>
      )}

      <div className="actions">
        <button onClick={props.onBack}>Back</button>
        <button
          className="primary"
          onClick={props.onNext}
          disabled={!r.can_launch || !allAcked || props.busy}
        >
          {r.needs_first_run.length > 0 ? "Start first run" : "Launch"}
        </button>
        {!allAcked && <span className="muted">acknowledge the warnings above to continue</span>}
      </div>
    </section>
  );
}

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
      <h2>First run</h2>
      <p className="note">
        Each CLI gets one bare pane in your project so logins and first-run “trust this folder”
        prompts are cleared before the team is built. Answer them in the terminal; this updates as
        you go.
      </p>
      <div className="actions">
        <button onClick={() => api.openTerminal(props.project).catch(() => {})}>
          Open terminal
        </button>
      </div>

      {props.panes.map((p) => (
        <div key={p.cli} className="panel">
          <header>
            <strong>{p.display_name}</strong>
            <span className={p.state === "verified" ? "ok" : "warn"}>
              {p.state === "verified"
                ? "ready"
                : p.state === "needs_you"
                  ? "waiting for you"
                  : "starting…"}
            </span>
          </header>
          {p.hints.length > 0 && (
            <ul className="hints">
              {p.hints.map((h) => (
                <li key={h.value}>
                  <span className="tag">{h.kind}</span>
                  <code>{h.value}</code>
                  <button onClick={() => navigator.clipboard.writeText(h.value)}>copy</button>
                </li>
              ))}
            </ul>
          )}
          {p.screen && <pre className="screen">{p.screen}</pre>}
        </div>
      ))}

      <div className="actions">
        <button className="primary" onClick={props.onDone} disabled={props.busy}>
          {allDone ? "Build the team" : "Skip and build anyway"}
        </button>
      </div>
    </section>
  );
}

function LaunchingStep(props: { progress: Progress[] }) {
  const last = props.progress.filter((p) => p.kind === "step").at(-1);
  const pct = last?.total ? Math.round(((last.index ?? 0) / last.total) * 100) : 0;
  return (
    <section>
      <h2>Building the team</h2>
      <div className="bar">
        <div style={{ width: `${pct}%` }} />
      </div>
      <ul className="log">
        {props.progress
          .filter((p) => p.kind !== "step")
          .map((p, i) => (
            <li key={i} className={p.kind}>
              <span className="tag">{p.kind.replace(/_/g, " ")}</span>
              {p.role && <strong>{p.role}</strong>}
              {p.detail && <span className="muted">{p.detail}</span>}
            </li>
          ))}
      </ul>
      {last?.detail && <p className="muted">{last.detail}</p>}
    </section>
  );
}

function DoneStep(props: {
  outcome: Outcome;
  project: string;
  setOutcome: (o: Outcome) => void;
  setError: (e: string | null) => void;
}) {
  const o = props.outcome;
  const release = (index: number) =>
    api
      .sendBriefingNow(index)
      .then(props.setOutcome)
      .catch((e) => props.setError(String(e)));

  return (
    <section>
      <h2>
        {o.briefed} of {o.panes.length} pane(s) briefed
      </h2>

      {o.failure && (
        <div className="error">
          <strong>Stopped:</strong> {o.failed_step}
          <div>{o.failure}</div>
          <p className="muted">
            Earlier panes were left running on purpose — they may hold real work.
          </p>
        </div>
      )}

      <ul className="list">
        {o.panes.map((p) => (
          <li key={p.index}>
            <strong>{p.role}</strong>
            <span className="muted">
              {p.agent_name ?? "—"} · {p.pane_id ?? "not created"}
            </span>
            {p.state === "briefed" ? (
              <span className="ok">briefed</span>
            ) : p.state === "needs_attention" ? (
              <span className="warn">{p.reason}</span>
            ) : (
              <span className="muted">{p.state.replace(/_/g, " ")}</span>
            )}
            {p.has_pending_briefing && (
              <button onClick={() => release(p.index)}>Send briefing now</button>
            )}
            {p.screen && p.state === "needs_attention" && <pre className="screen">{p.screen}</pre>}
          </li>
        ))}
      </ul>

      <div className="actions">
        <button
          className="primary"
          onClick={() =>
            api.openTerminal(props.project).catch((e) => props.setError(String(e)))
          }
        >
          Open terminal
        </button>
        <code>herdr --session {o.session}</code>
      </div>
    </section>
  );
}
