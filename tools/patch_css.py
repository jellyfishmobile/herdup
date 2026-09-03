"""Style the second thesis heading, and bump the stylesheet version."""

from pathlib import Path

SITE = Path(__file__).resolve().parents[1] / "site"

css_path = SITE / "styles.css"
css = css_path.read_text(encoding="utf-8")
if ".sub-thesis" not in css:
    css += """

/* The category claim leads; the attention argument is the deeper second beat.
   It reads as a heading in its own right rather than competing with the h2. */
.sub-thesis {
  font-size: clamp(21px, 2.6vw, 28px);
  letter-spacing: -0.03em;
  font-weight: 700;
  margin: 36px 0 14px;
  padding-top: 28px;
  border-top: 1px solid var(--rule);
  text-wrap: balance;
}
"""
    css_path.write_text(css, encoding="utf-8")

html_path = SITE / "index.html"
html = html_path.read_text(encoding="utf-8")
html = html.replace("styles.css?v=21", "styles.css?v=22")
html_path.write_text(html, encoding="utf-8")

print("sub-thesis styled:", ".sub-thesis" in css_path.read_text(encoding="utf-8"))
print("css version bumped:", "styles.css?v=22" in html)
