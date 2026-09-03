"""Let added roles carry a tool, so the CLI picker can cover every teammate.

`overrides` is keyed on TEMPLATE indices, so it can never reach a role the user
added -- those have no template index. Without this, a tool picker would work
for four teammates and silently ignore the two you added, which is worse than
not having one.

`extra` entries stay plain ids, or become "id:cli". No colon means the role's
default tool, so every existing caller keeps working.
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
lib = ROOT / "app" / "src-tauri" / "src" / "lib.rs"
src = lib.read_text(encoding="utf-8")

OLD = """    for id in &options.extra {
        let role = addable
            .iter()
            .find(|r| &r.id == id)
            .ok_or_else(|| format!("no role '{id}' to add"))?;
        request = request.add_pane(role.spec.clone());
    }"""

NEW = """    for entry in &options.extra {
        // "coder" uses the role's default tool; "coder:agy" overrides it.
        // Overrides are keyed on template indices and so can never address an
        // added role, which is why the tool travels with the entry instead.
        let (id, cli) = match entry.split_once(':') {
            Some((id, cli)) => (id, Some(cli)),
            None => (entry.as_str(), None),
        };
        let role = addable
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| format!("no role '{id}' to add"))?;
        let mut spec = role.spec.clone();
        if let Some(cli) = cli {
            if !registry.contains(cli) {
                return Err(format!("no tool '{cli}' in the registry"));
            }
            spec.cli = cli.to_string();
            // The role's default flags belong to its default tool. Carrying
            // them onto a different CLI is exactly the bug the plan builder
            // already guards against for template panes.
            spec.flags = String::new();
        }
        request = request.add_pane(spec);
    }"""

assert OLD in src, "extra-resolution block not found"
src = src.replace(OLD, NEW)
lib.write_text(src, encoding="utf-8")
print("extra accepts id:cli:", "split_once(':')" in src)

# --- the front end contract -------------------------------------------------
api = ROOT / "app" / "src" / "api.ts"
a = api.read_text(encoding="utf-8")
a = a.replace(
    "  /// Ids from `listAddableRoles`, in the order they were added.\n  extra: string[];",
    "  /// Ids from `listAddableRoles`, in the order they were added.\n"
    '  /// `"coder"` uses the role default tool; `"coder:agy"` overrides it.\n'
    "  extra: string[];",
)
api.write_text(a, encoding="utf-8")
print("api documented:", "coder:agy" in a)
