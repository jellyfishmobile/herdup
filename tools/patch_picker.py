"""Re-add the per-teammate tool picker.

The old team screen had a CLI dropdown in every row. The redesign replaced that
table with the workspace picture and never re-added the control, so the only
way to reach a tool other than Claude Code was the preflight remediation that
fires when one is missing. That is a regression, not a design decision.

Put back behind one click: the picture stays the primary thing, "Change tools"
opens a compact row per teammate. Template panes go through `overrides` (keyed
on the template index); added roles carry their tool in the `extra` entry,
because overrides cannot address a pane the template never had.
"""

from pathlib import Path

APP = Path(__file__).resolve().parents[1] / "app" / "src" / "App.tsx"
src = APP.read_text(encoding="utf-8")

# --- props ------------------------------------------------------------------
src = src.replace(
    """function TeamStep(props: {
  project: string;
  templates: Template[];
  addable: AddableRole[];
  plan: Plan | null;""",
    """function TeamStep(props: {
  project: string;
  templates: Template[];
  addable: AddableRole[];
  clis: Cli[];
  plan: Plan | null;""",
    1,
)
src = src.replace(
    """  setSkip: (f: (s: number[]) => number[]) => void;
  onBack: () => void;""",
    """  setSkip: (f: (s: number[]) => number[]) => void;
  setOverrides: (f: (o: [number, string][]) => [number, string][]) => void;
  onBack: () => void;""",
    1,
)

# --- pass them in -----------------------------------------------------------
src = src.replace(
    """            templates={templates}
            addable={addable}
            plan={plan}""",
    """            templates={templates}
            addable={addable}
            clis={clis}
            plan={plan}""",
    1,
)
src = src.replace(
    """            extra={extra}
            setExtra={setExtra}
            setSkip={setSkip}""",
    """            extra={extra}
            setExtra={setExtra}
            setSkip={setSkip}
            setOverrides={setOverrides}""",
    1,
)

# --- the control -------------------------------------------------------------
OLD_ADD = """      <div className="add">
        {ordered(props.addable).map((r) => ("""
NEW_ADD = """      {/* Kept behind a click: most teams are one tool, and the picture is the
          point of this screen. One click away is not hidden. */}
      <details className="tools">
        <summary>
          Change tools
          <span className="muted">
            {" · "}
            {distinctTools.length === 1 ? distinctTools[0] : `${distinctTools.length} tools`}
          </span>
        </summary>
        <ul className="toollist">
          {panes.map((p, i) => (
            <li key={`${p.role}-${i}`}>
              <span className="grow">{p.role}</span>
              <select
                aria-label={`Tool for ${p.role}`}
                data-testid={`tool-${i}`}
                value={p.cli}
                onChange={(e) => setTool(i, e.target.value)}
              >
                {props.clis.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.display_name}
                  </option>
                ))}
              </select>
            </li>
          ))}
        </ul>
        <p className="muted">
          Every teammate can run a different tool. herdup drops any permission flags the new tool
          isn&apos;t known to accept, rather than passing them on and hoping.
        </p>
      </details>

      <div className="add">
        {ordered(props.addable).map((r) => ("""
assert OLD_ADD in src, "add-pill block not found"
src = src.replace(OLD_ADD, NEW_ADD, 1)

# --- the logic ---------------------------------------------------------------
OLD_REMOVE = "  const removeAt = (paneIndex: number) => {"
NEW_LOGIC = """  const distinctTools = [...new Set(panes.map((p) => p.cli_display))];

  /// Change one teammate's tool.
  ///
  /// A template pane goes through `overrides`, which is keyed on the TEMPLATE
  /// index — the compacted index shifts as panes are dropped. An added role has
  /// no template index, so its tool travels with its `extra` entry instead.
  const setTool = (paneIndex: number, cli: string) => {
    const pane = panes[paneIndex];
    if (!pane) return;
    if (pane.origin !== null) {
      props.setOverrides((o) => [...o.filter(([i]) => i !== pane.origin), [pane.origin!, cli]]);
      return;
    }
    const addedBefore = panes.slice(0, paneIndex).filter((p) => p.origin === null).length;
    props.setExtra((e) =>
      e.map((entry, i) => (i === addedBefore ? `${entry.split(":")[0]}:${cli}` : entry)),
    );
  };

  const removeAt = (paneIndex: number) => {"""
assert OLD_REMOVE in src, "removeAt not found"
src = src.replace(OLD_REMOVE, NEW_LOGIC, 1)

APP.write_text(src, encoding="utf-8")
print("picker added:", 'data-testid={`tool-' in src)
print("setTool handles both:", "addedBefore" in src and "setOverrides" in src)
