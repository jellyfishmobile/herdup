# Phase 0 — herdr ground truth

**Date:** 2026-09-02
**herdr version:** `0.8.2-preview.2026-08-31-b1ff4582e968` (Windows x86_64)
**Method:** isolated named session (`herdr --session herdup-spike server`), destroyed afterwards
**Fixtures:** [`tests/fixtures/herdr/`](../../tests/fixtures/herdr/)

**Outcome: the spec's central safety guarantee is disproven.** Details in §4.
Phase 1 is blocked until the spec is revised.

---

## 1. Version drift

The spec was designed against herdr **0.7.0** source. The installer ships
**0.8.2-preview**, and Windows installs default to the preview channel. Several
documented behaviours have changed (§3, §5). All findings below are 0.8.2.

## 2. Isolation — how to test without touching a live session

A herdr server was already running on this machine owning real user panes.
`herdr server stop` exits every pane process, so it was never an option.

`HERDR_SOCKET_PATH` alone is **not sufficient**: it redirects the socket but the
server still reads the shared state directory, so a server started that way
restored a duplicate of the live session's panes.

The correct isolation is a **named session**, which gets its own `socket_path`
*and* `session_dir`:

```
herdr --session herdup-spike server      # own dir: %APPDATA%\herdr\sessions\herdup-spike
herdr --session herdup-spike <command>
herdr session stop herdup-spike && herdr session delete herdup-spike
```

Use this for every test and for CI.

An accidental safeguard also applied: a 0.8.2 client refuses to talk to a 0.7.x
server (`protocol_mismatch`, client 21 vs server 20), so the live session was
unreachable even by mistake.

## 3. Confirmed and corrected API facts

**Confirmed** — every JSON path the spec relies on exists:

| Command | Path | Value observed |
|---|---|---|
| `workspace create` | `result.workspace.workspace_id` | `w1` |
| `workspace create` | `result.tab.tab_id` | `w1:t1` |
| `workspace create` | `result.root_pane.pane_id` | `w1:p1` |
| `pane split` | `result.pane.pane_id` | `w1:p2` |

**Corrected:**

1. **ID format changed.** 0.7.0 docs say workspace `1`, tab `1:1`, pane `1-1`.
   0.8.2 uses `w1`, `w1:t1`, `w1:p1`. Never construct IDs; always parse them.
2. **Pane IDs are monotonic — they do not compact.** Closing `w1:p2` and
   re-splitting allocated `w1:p4`, not a reused `w1:p2`. The compaction hazard in
   spec §9 and the teardown-then-re-read ordering rule **do not apply to 0.8.x**.
3. **`pane rename` returns JSON** (a `pane_info` result). The 0.7.0 docs omit it
   from the JSON-returning list.
4. Pane numbering is **workspace-scoped**, not tab-scoped: a pane created in a
   second tab was `w1:p3`.
5. `cwd` is returned with a trailing separator in `workspace create`
   (`D:\work\herdr_automation\`) but without one in `pane get`. Normalise before
   comparing.

## 4. The critical finding: `idle` does not mean "safe to brief"

Spec §5 rests on one claim — a CLI awaiting human input never reports `idle`, so a
briefing can only ever be typed at a real prompt. **That claim is false.**

### 4.1 Claude Code behaves as designed

| t | `agent_status` | Screen |
|---|---|---|
| 0.0s | `unknown` | shell |
| 1.0s | `unknown` | agent identified as `claude` |
| 4.7s | **`blocked`** | *"Is this a project you trust?"* |
| +1.0s | **`idle`** | prompt box, after answering |

Correct throughout. A briefing would have been withheld and then released.

### 4.2 Gemini CLI reports `idle` while blocked

Launched in the same folder:

| t | `agent_status` | Screen |
|---|---|---|
| 4.7s | **`idle`** | *"1. Trust folder / 2. Trust parent folder / 3. Don't trust"* |

herdr reported `idle` while a blocking modal was on screen. Sending the briefing
exactly as Stage 2 would:

```
pane send-text w1:p4 "You are the Researcher for this repo..."
pane send-keys w1:p4 Enter
```

produced:

```
Gemini CLI is restarting to apply the trust changes...
```

**The briefing never reached the agent, and the trailing Enter selected
"1. Trust folder" — silently granting a security permission on the user's
behalf.** Gemini's config can execute code, so this is not a cosmetic failure.

### 4.3 Why

herdr's README grades its agents: most are fully supported, but *"detected but not
fully tested: gemini cli, cline."* Detection quality is **per-agent**, so a
guarantee derived from `agent_status` is only as strong as that agent's manifest.
The spec treated it as universal.

### 4.4 Required change

The briefing-safety guarantee must become **per-CLI, not global**. Proposed:
a `briefing_trust` tier on each registry entry.

- **`verified`** — blocked-detection confirmed by our own testing. Auto-brief on
  `idle`. Claude Code qualifies today.
- **`manual`** — default for everything else, including every agent herdr lists as
  not fully tested. Never auto-brief. Show the pane's screen and require a human
  click on *Send briefing now*.

A CLI is promoted to `verified` only after someone reproduces §4.1's transition
for it. Never by assumption. This mirrors the existing rule for flag presets:
ship blank, fill in what is proven.

## 5. First-run interstitials are normal, not exceptional

Both Claude Code and Gemini CLI show a trust-this-folder prompt on first launch in
an unfamiliar directory. This is the **common** case for the "create a new repo,
launch a team into it" flow, not an edge case.

The decision persists per folder: a second `claude` in the same folder went
`unknown → idle` in 4.7s with no prompt.

Consequence: in a new repo, the **first** pane of each CLI blocks on trust and
later panes do not. The UI must present this as an expected step, not an error.
Handling one trust prompt per CLI per project is exactly what Stage 1 already
does for logins — Stage 1 should be widened from "sign-in" to "first-run", and
running it in the **target project directory** would clear both.

## 6. Timings

- Agent identified ~1.0s after `pane run`.
- Terminal state settles ~4.7s after launch, consistently across three launches.
- `blocked → idle` within ~1.0s of the human answering.

The spec's 30s `AwaitIdle` timeout is generous; keep it — a cold npm shim or a
slow machine will be slower than this one.

## 7. Registry corrections

`claude` resolves to `C:\Users\ronal\.local\bin\claude.exe` — a native binary, not
the `claude.cmd` npm shim the spec's registry assumed. Installed here:

| CLI | Path |
|---|---|
| claude | `~\.local\bin\claude.exe` |
| gemini | `~\AppData\Roaming\npm\gemini.ps1` |
| kimi | `~\.kimi-code\bin\kimi.exe` |
| hermes | `~\AppData\Local\hermes\hermes-agent\venv\Scripts\hermes.exe` |

Three different install shapes across four CLIs. **Do not hardcode binary
filenames.** Resolve by base name via `where`/`which` and store the resolved
absolute path; keep `binary` as a base name only.

## 8. Actions

Spec changes required before Phase 1:

- [ ] §5 — replace the universal guarantee with the `briefing_trust` tiering (§4.4).
- [ ] §7.1 — add `briefing_trust`; change `binary` to a base name resolved at preflight.
- [ ] §8 — widen Stage 1 from "sign-in" to "first-run", run it in the target project dir.
- [ ] §9 + §8 Stage 1 — drop the ID-compaction warning and the teardown ordering rule;
      pin minimum herdr **0.8.2**.
- [ ] §11 — first-run trust prompts are an expected state, not an error.
- [ ] §13 — record the 0.7.0-vs-0.8.2 drift; the spec's source-derived assumptions
      were one minor version stale.

Plan changes:

- [ ] Phase 2 — every registry entry ships `briefing_trust = "manual"` except `claude`.
- [ ] Phase 4 — add a test: a `manual`-tier CLI reporting `idle` must **not** be auto-briefed.
- [ ] All phases — use a named session for tests; never the default session.
