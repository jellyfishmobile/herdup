# herdr 0.8.2 has an agent API — and the design does not use it

**Date:** 2026-09-02 (found during Phase 6)
**Status:** blocks Phase 6 completion; needs a spec decision before rework
**Evidence:** [`herdr-0.8.2-skill.md`](herdr-0.8.2-skill.md), captured from `herdr --skill`

## What happened

The first real end-to-end launch built its pane, ran Claude Code, correctly
withheld the briefing, and reported *"the CLI did not reach its prompt in time"*.
Investigating that wording surfaced the actual cause:

```
$ herdr wait agent-status w1:p1 --status idle --timeout 4000
unknown command: wait
exit=2   elapsed=0.12s
```

**`wait` is not a command on herdr 0.8.2.** The executor had been calling a
non-existent command since Phase 4, receiving exit 2, and treating it as "the
wait timed out". The resulting *behaviour* was still safe — a briefing is
withheld unless a pane is observed ready, and an unobservable pane is never
ready — but nothing was ever actually waiting, and the reported reason was wrong.

## Why it survived three phases

Two mistakes, both mine:

1. **Phase 0 did not capture `wait`.** The plan listed it among the commands to
   record. I polled `pane get` in a loop by hand instead and never noticed the
   substitution, so no fixture ever proved the command existed.
2. **Exit 2 was folded into a normal outcome.** herdr documents exit 2 as a CLI
   *syntax* error, which is always a caller bug. Treating any non-zero exit as
   "timed out" made a broken call indistinguishable from a working one.

Fixed: `HerdrError::CliSyntax` now surfaces exit 2 loudly on every command path,
with regression tests. The failing launch reports the real problem in one line.

**Root cause behind both:** the design was written against herdr **0.7.0 source**
read from a checkout, while the installed binary is **0.8.2-preview**. The
binary ships `herdr --skill`, which prints the current agent skill file — the
authoritative CLI surface. Reading a vendored copy of an older version instead
of asking the installed binary is what made every downstream assumption stale.

## What 0.8.2 actually offers

An agent-level API that did not exist in the 0.7.0 docs:

| Command | Behaviour |
|---|---|
| `agent start <name> --kind KIND --pane ID [--timeout MS] [-- args…]` | Starts an agent and **returns only once herdr sees it ready for input**. Returns `agent_not_ready` if it blocks during startup. Default 30 s. |
| `agent prompt <name> <text> [--wait] [--until STATUS] [--timeout MS]` | **"Rejects an agent already waiting at an approval or question dialog with `agent_blocked` before sending any input."** |
| `agent wait <name> [--until STATUS] [--timeout MS]` | Waits for a settled state. |
| `agent get` / `agent read` / `agent list` / `agent send-keys` | Agent-resolved equivalents of the pane commands. |
| `pane wait-output <id> --match TEXT [--regex P] [--timeout MS]` | The real name; not `wait output`. |

23 supported kinds: `pi, claude, codex, gemini, cursor, devin, agy, cline, omp,
mastracode, opencode, copilot, kimi, kiro, droid, amp, grok, hermes, kilo,
qodercli, qwen, maki, muse`.

Two further confirmations from the same file: closed pane IDs are **not reused**
(matching the Phase 0 monotonic finding), and CLI server errors are **JSON on
stderr with exit 1** (matching the Phase 1 finding). Both were guesses we
verified experimentally; they are documented behaviour.

## Why this matters beyond a bug fix

**`agent prompt` implements herdup's central safety property natively.** Spec §5
exists because a briefing typed into a blocking dialog answers that dialog —
which is exactly what happened to Gemini in Phase 0. herdr now refuses to send
input to an agent at an approval or question dialog, *before writing any bytes*.

That is strictly better than herdup's own gate, because it is enforced at the
point of writing rather than inferred a moment earlier from a separate status
read.

**It does not make §5.1's `briefing_trust` tiering redundant.** `agent_blocked`
depends on the same per-agent detection that failed for Gemini: if a CLI reports
`idle` while a modal is up, `agent prompt` will send. The tiering remains the
outer layer, and herdr's check becomes an inner one. Defence in depth, not
duplication.

## Options

1. **Adopt the agent API** (recommended). `agent start` replaces
   `RunCommand` + `AwaitIdle`; `agent prompt --wait` replaces `send-text` +
   `send-keys Enter`. Less code, fewer round trips, and the safety property is
   enforced by herdr as well as by us. Costs: the registry needs a `kind` field
   mapped to herdr's list; `flags` must move after `--`; `Step` changes shape;
   plan/executor tests need reworking. Spec §5, §6.2 and §8 need edits.
2. **Minimal fix.** Replace `wait agent-status` with `agent wait`, keep
   `pane run` and `send-text`. Smaller diff, but keeps herdup driving raw
   terminals when herdr offers a validated agent surface, and forgoes the
   `agent_blocked` guarantee.
3. **Ship Phase 6 with waiting disabled.** Not recommended: without a wait,
   every pane is withheld and the product does nothing useful unattended.

## Actions

- [x] `CliSyntax` error so exit 2 can never hide again; regression tests added.
- [x] Capture `herdr --skill` output into this directory as the authority.
- [x] **Option 1 chosen and implemented** (2026-09-02). See below.
- [ ] Re-baseline the rest of the spec against 0.8.2 rather than 0.7.0 source.
- [ ] Amend the plan's Phase 0 to require `herdr --skill` first, and to fail the
      phase if any command it lists was not actually executed.

## Outcome

Adopted. Real JSON was captured first this time
(`tests/fixtures/herdr/agent_*.json`) rather than coding against prose.

What the captures added beyond the docs:

- `agent get` returns **`interactive_ready`**, an explicit boolean. Stronger than
  inferring readiness from `agent_status`, and now what gates every keystroke.
- `agent start` returned `agent_not_ready` in **4.1 s** against a real trust
  prompt — it detects the block immediately rather than waiting out its timeout.
- A **new failure mode the docs do not mention**: `agent_pane_busy`, *"pane w1:p1
  is not an available shell"*. A pane created moments earlier is not always at
  its prompt. This is a genuine race — the same launch succeeded once and failed
  the next time. Handled with a bounded retry (10 × 400 ms).

Two further corrections the rework forced:

- The **coordinator briefing itself named `herdr wait agent-status`** — the same
  non-existent command, about to be handed to every coordinator as an
  instruction. It now names only verified commands, addresses teammates by agent
  name, and tells the coordinator what to do when herdr answers `agent_blocked`.
  A test asserts the briefing contains no command herdr lacks.
- The registry gained `kind`, and with it six agent kinds herdup could not
  previously launch at all: **`agy`**, `omp`, `mastracode`, `qwen`, `maki`,
  `muse`. Tests pin every declared kind to herdr's list and assert every kind
  herdr accepts is reachable.

`antigravity` has a detection manifest but is not an `agent start` kind, so it
falls back to raw pane commands — and therefore can never be auto-briefed, since
herdr's `agent_blocked` guard is unavailable for it.

**Verified end to end 2026-09-02:** a full six-agent team launched in 75.5 s,
exit 0, all six briefed, coordinator briefed last with a roster naming every
teammate's agent name and pane.

## Method note: `cargo run` hides the exit

Running a launch through `cargo run` appeared to hang for ten minutes. The built
binary run directly exits in well under a second. The hang is in `cargo run`
holding the process tree, not in herdup — an earlier "fix" to detach the spawned
server was aimed at the wrong target. **Test launches with
`target/debug/launcher-cli.exe`, not `cargo run`.**
