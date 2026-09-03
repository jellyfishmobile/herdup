"""Ship mimo without permission presets, and update the registry counts.

config.rs states the rule plainly: "A wrong permission flag fails silently and
could disable a sandbox, so presets are only shipped where verified." Only
claude ships presets, because only claude has been run and verified.

mimo's flags are real — they come from its own --help — but
`--dangerously-skip-permissions` is exactly the class of flag that rule exists
to stop us shipping on documentation alone. Nobody has yet run mimo under
herdup and watched what it does. So it ships with no presets; the flag is
recorded in a comment for whoever verifies it, and a user can add it to their
own registry.toml today.
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

reg = ROOT / "crates" / "launcher-core" / "assets" / "registry.toml"
text = reg.read_text(encoding="utf-8")
text = text.replace(
    'flag_presets = ["--dangerously-skip-permissions --trust", "--trust", ""]',
    "# No presets until someone has actually run mimo under herdup: an unverified\n"
    "# permission flag fails silently and could disable a sandbox (see config.rs).\n"
    "# Its --help documents `--dangerously-skip-permissions` (alias --yolo) and\n"
    "# `--trust`; add them in your own registry.toml if you want them.\n"
    'flag_presets = [""]',
)
reg.write_text(text, encoding="utf-8")

# Adding a CLI legitimately moves these counts.
cfg = ROOT / "crates" / "launcher-core" / "tests" / "config.rs"
c = cfg.read_text(encoding="utf-8")
before = c
c = c.replace("        24\n", "        25\n").replace(", 24)", ", 25)").replace("== 24", "== 25")
c = c.replace("        25 + 1\n", "        26\n")
# The user-added-CLI tests assert builtin + 1.
c = c.replace("        25,\n", "        26,\n")
c = c.replace("        24,\n", "        25,\n")
cfg.write_text(c, encoding="utf-8")

print("mimo presets cleared:", 'flag_presets = [""]' in text.split("[mimo]")[1])
print("config.rs touched:", c != before)
