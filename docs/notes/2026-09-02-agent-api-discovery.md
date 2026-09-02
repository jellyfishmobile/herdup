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
- [ ] **Decide between option 1 and 2 before continuing Phase 6.**
- [ ] Re-baseline the spec against 0.8.2 rather than 0.7.0 source.
- [ ] Amend the plan's Phase 0 to require `herdr --skill` first, and to fail the
      phase if any command it lists was not actually executed.
