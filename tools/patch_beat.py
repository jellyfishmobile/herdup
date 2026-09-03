"""Move the first beat directly under the hero.

It is the line that explains the picture above it, so it should not wait until
the reader has scrolled past a WebGL section to arrive. The remaining beats
keep the herd field; beat 01 becomes a standalone band between the hero and
the story.
"""

from pathlib import Path

SITE = Path(__file__).resolve().parents[1] / "site"
html = (SITE / "index.html").read_text(encoding="utf-8")

BEAT1 = """      <div class="beat">
        <div class="beat-in">
          <span class="step">01</span>
          <h2>Four agents is not four times one.</h2>
          <p>
            Four terminals. Four sets of permission flags. The same &ldquo;do you trust this folder?&rdquo;
            question, four times. Then you brief each one by hand and hope you said the same thing
            twice. Most people give up before finding out whether a team beats a single agent.
          </p>
        </div>
      </div>

"""

# The entity-escaped variant is what is actually on disk.
if BEAT1 not in html:
    start = html.index('<span class="step">01</span>')
    open_div = html.rindex('<div class="beat">', 0, start)
    close = html.index("</div>\n      </div>\n", start) + len("</div>\n      </div>\n")
    BEAT1 = html[open_div - 6 : close]

html = html.replace(BEAT1, "", 1)

BAND = """    <!-- The line that explains the picture above it, so it sits directly
         under the hero rather than behind a WebGL section. -->
    <section class="opening">
      <div class="opening-in rise">
        <span class="step">01</span>
        <h2>Four agents is not four times one.</h2>
        <p>
          Four terminals. Four sets of permission flags. The same &ldquo;do you trust this
          folder?&rdquo; question, four times. Then you brief each one by hand and hope you said the
          same thing twice. Most people give up before finding out whether a team beats a single
          agent.
        </p>
      </div>
    </section>

"""

anchor = "    <!-- ============================================================\n         The story."
html = html.replace(anchor, BAND + anchor, 1)
html = html.replace("styles.css?v=23", "styles.css?v=24")
html = html.replace("scroll.js?v=12", "scroll.js?v=13")

(SITE / "index.html").write_text(html, encoding="utf-8")

css_p = SITE / "styles.css"
css = css_p.read_text(encoding="utf-8")
if ".opening" not in css:
    css += """

/* The first beat, promoted out of the scroll canvas to sit under the hero. */
.opening { max-width: 900px; margin: 0 auto; padding: 54px 24px; border-bottom: 1px solid var(--rule); }
.opening-in { max-width: 52ch; }
.opening .step {
  display: inline-block; font-size: 11px; letter-spacing: 0.24em;
  color: var(--acc); font-weight: 500; margin-bottom: 12px;
}
.opening h2 { font-size: clamp(24px, 3.2vw, 34px); margin-bottom: 12px; }
.opening p { color: var(--dim); font-size: 16px; }
"""
    css_p.write_text(css, encoding="utf-8")

print("beat 01 moved:", '<section class="opening">' in html)
print("beats left in story:", html.count('<div class="beat">'))
print("opening styled:", ".opening" in css_p.read_text(encoding="utf-8"))
