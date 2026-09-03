"""Update the registry count assertions for the added CLI, correctly.

Five sites, and they do not all mean the same thing:

  line  74  the builtin registry itself                -> builtin count
  line 321  builtins plus one user-defined CLI         -> builtin + 1
  line 597  fallback when the config dir is missing    -> builtin count
  line 606  fallback when the config dir is empty      -> builtin count
  line 625  user files merged on top of the builtins   -> builtin + 1

A blanket find-and-replace conflates the two groups, which is exactly what I
did a moment ago. The "+1" sites are rewritten to say what they mean, so they
stop needing an edit every time a CLI is added; line 74 keeps a literal,
because that one IS the assertion that the registry has not silently lost or
gained an entry.
"""

from pathlib import Path

p = Path(__file__).resolve().parents[1] / "crates" / "launcher-core" / "tests" / "config.rs"
lines = p.read_text(encoding="utf-8").splitlines(keepends=True)


def set_line(idx0: int, text: str) -> None:
    indent = len(lines[idx0]) - len(lines[idx0].lstrip())
    lines[idx0] = " " * indent + text + "\n"


# 1-indexed line numbers from the grep above.
set_line(73, "assert_eq!(reg.len(), 25, \"a CLI was added or lost without updating this\");")
set_line(320, "assert_eq!(reg.len(), Registry::builtin().len() + 1);")
set_line(596, "assert_eq!(reg.len(), Registry::builtin().len());")
set_line(605, "assert_eq!(reg.len(), Registry::builtin().len());")
set_line(624, "assert_eq!(reg.len(), Registry::builtin().len() + 1);")

p.write_text("".join(lines), encoding="utf-8")
for n in (74, 321, 597, 606, 625):
    print(f"{n}: {lines[n - 1].strip()}")
