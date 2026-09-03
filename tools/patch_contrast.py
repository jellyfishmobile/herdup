"""Make the herd field visible in light mode.

The particle colours were chosen when the page was dark: #6c7180 dots at low
opacity read fine on #131417 and disappear on #f8f8f7. The story-scrim then
paints near-white on top of them, so in light mode the section rendered as an
empty page — a visitor genuinely cannot tell there is anything there.

Fix: take the colours from the CSS custom properties instead of hard-coding
them, so they follow the theme by construction and cannot drift again. Also
lift the opacity floor and ease off the scrim.
"""

from pathlib import Path

SITE = Path(__file__).resolve().parents[1] / "site"

js = SITE / "scroll.js"
src = js.read_text(encoding="utf-8")

OLD = '''  const colors = new Float32Array(COUNT * 3);
  const amber = new THREE.Color("#d9903f");
  const dim = new THREE.Color("#6c7180");'''
NEW = '''  // Colours come from the stylesheet, never hard-coded. They were hard-coded
  // once, chosen against a dark page, and when the page went light the field
  // became invisible — the section read as blank. Reading the tokens means the
  // field is correct in both themes by construction.
  const css = getComputedStyle(document.documentElement);
  const token = (name, fallback) => (css.getPropertyValue(name).trim() || fallback);
  const colors = new Float32Array(COUNT * 3);
  const amber = new THREE.Color(token("--acc", "#b8681e"));
  const dim = new THREE.Color(token("--dim", "#55585d"));'''
assert OLD in src, "colour block not found"
src = src.replace(OLD, NEW)

# A floor of 0.35 is too faint against a light ground; and the dots were small.
src = src.replace("    size: 0.13,", "    size: 0.16,")
src = src.replace(
    "    material.opacity = 0.35 + t * 0.5;",
    "    // Never fully faint: on a light ground a low floor reads as nothing.\n"
    "    material.opacity = 0.55 + t * 0.4;",
)
js.write_text(src, encoding="utf-8")

css_p = SITE / "styles.css"
css = css_p.read_text(encoding="utf-8")
css += """

/* The scrim exists to keep the sentence legible, not to erase the herd. On a
   light ground the old strength washed the field out completely. */
.story-scrim {
  background: linear-gradient(
    90deg,
    var(--pa) 0%,
    color-mix(in srgb, var(--pa) 86%, transparent) 22%,
    color-mix(in srgb, var(--pa) 40%, transparent) 42%,
    transparent 60%
  );
}
@media (max-width: 860px) {
  .story-scrim { background: color-mix(in srgb, var(--pa) 62%, transparent); }
}
"""
css_p.write_text(css, encoding="utf-8")

print("colours from tokens:", "--acc" in src and "getPropertyValue" in src)
print("opacity floor raised:", "0.55 + t * 0.4" in src)
print("scrim eased:", css.count(".story-scrim") >= 2)
