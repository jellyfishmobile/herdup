"""Repair the duplicate docs_url my last edit introduced on [opencode].

Two keys of the same name is invalid TOML and would have failed at load. The
registry already carried a different URL (anomalyco/opencode); the user gave
opencode-ai/opencode, so that wins and the old line goes.
"""

from pathlib import Path

reg = Path(__file__).resolve().parents[1] / "crates" / "launcher-core" / "assets" / "registry.toml"
lines = reg.read_text(encoding="utf-8").splitlines(keepends=True)

out, section, seen = [], None, set()
for line in lines:
    stripped = line.strip()
    if stripped.startswith("[") and stripped.endswith("]"):
        section, seen = stripped, set()
        out.append(line)
        continue
    key = stripped.split("=", 1)[0].strip() if "=" in stripped else None
    if key and section == "[opencode]":
        if key in seen:
            # Keep the first occurrence, which is the URL the user supplied.
            continue
        seen.add(key)
    out.append(line)

reg.write_text("".join(out), encoding="utf-8")

text = reg.read_text(encoding="utf-8")
block = text.split("[opencode]")[1].split("\n[")[0]
print("opencode block:")
print("[opencode]" + block.rstrip())
print("docs_url count:", block.count("docs_url"))
