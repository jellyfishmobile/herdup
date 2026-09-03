"""Add Xiaomi mimo to the registry, and the opencode docs link.

mimo deliberately has NO `kind`. herdr 0.8.2 lists its supported agent kinds as
pi, claude, codex, gemini, cursor, devin, agy, cline, omp, mastracode,
opencode, copilot, kimi, kiro, droid, amp, grok, hermes, kilo, qodercli, qwen,
maki, muse — and a direct probe returns `unsupported interactive agent kind:
mimo`. Verified, not assumed.

Without a kind, herdup starts it with RunCommand in a plain pane. There is no
readiness signal and no `agent_blocked` guard on that path, so the plan builder
forces BriefingGate::RequiresHuman: you will be asked to release its
instructions yourself after looking at the pane. That is the correct behaviour,
not a limitation to paper over — the whole guard exists because a briefing was
once fired into a tool sitting at a trust prompt.

Flags come from `mimo --help`:
  --dangerously-skip-permissions, --yolo  auto-approve permissions (dangerous)
  --trust                                 skip the workspace trust prompt
"""

from pathlib import Path

reg = Path(__file__).resolve().parents[1] / "crates" / "launcher-core" / "assets" / "registry.toml"
text = reg.read_text(encoding="utf-8")

if "[mimo]" not in text:
    text = text.rstrip() + """

[mimo]
display_name = "MiMo Code"
binary = "mimo"                                     # Xiaomi mimocode
# No `kind`: herdr 0.8.2 has no mimo agent kind (probe: "unsupported
# interactive agent kind: mimo"). It therefore runs as a plain command and can
# never be briefed unattended — there is no readiness signal to gate on.
docs_url = "https://github.com/XiaomiMiMo/MiMo"
flag_presets = ["--dangerously-skip-permissions --trust", "--trust", ""]
briefing_trust = "manual"
"""

# The user supplied the canonical opencode repo.
if 'docs_url = "https://github.com/opencode-ai/opencode"' not in text:
    text = text.replace(
        '[opencode]\ndisplay_name = "opencode"\nbinary = "opencode"',
        '[opencode]\ndisplay_name = "opencode"\nbinary = "opencode"\n'
        'docs_url = "https://github.com/opencode-ai/opencode"',
        1,
    )

reg.write_text(text, encoding="utf-8")
print("mimo added:", "[mimo]" in text)
print("mimo has no kind:", "kind = \"mimo\"" not in text)
print("opencode docs:", "opencode-ai/opencode" in text)
