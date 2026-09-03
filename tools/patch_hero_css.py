"""Invert the hero treatment for a light illustration, and clear the last
reference to the old dark plate."""

from pathlib import Path

SITE = Path(__file__).resolve().parents[1] / "site"

html_p = SITE / "index.html"
html = html_p.read_text(encoding="utf-8")
# The lazy-loader still probed the old file to decide whether to swap.
html = html.replace('fetch("/img/hero.mp4"', 'fetch("/img/hero-factory.mp4"')
html_p.write_text(html, encoding="utf-8")

css_p = SITE / "stage.css"
css = css_p.read_text(encoding="utf-8")
css += """

/* ==================================================================
   Hero, light treatment
   ------------------------------------------------------------------
   The hero art is now a light flat-vector illustration, not a dark
   photograph, so every rule that assumed white-on-dark is inverted:
   dark type, a white scrim on the reading side only, and the right of
   the frame left clear so the factory is actually visible.
   ================================================================== */

.hero { background: #ffffff; }
.hero .hero-media img,
.hero .hero-media video { object-fit: cover; object-position: 62% 50%; }

/* Legibility floor for the copy, without washing out the artwork. */
.hero .hero-media::after {
  background: linear-gradient(
    100deg,
    rgba(255, 255, 255, 0.97) 0%,
    rgba(255, 255, 255, 0.9) 26%,
    rgba(255, 255, 255, 0.55) 44%,
    rgba(255, 255, 255, 0.08) 62%,
    rgba(255, 255, 255, 0) 78%
  );
}

.hero-copy { color: #16171a; }
.hero-copy h1 { color: #101114; text-shadow: none; max-width: 13ch; }
.hero-copy .lede { color: #3f434a; max-width: 42ch; }
.hero-copy .fineprint { color: #6b7078; }
.hero-copy .fineprint a { color: #a85d19; }

.hero .btn {
  background: rgba(255, 255, 255, 0.85);
  border-color: rgba(0, 0, 0, 0.18);
  color: #16171a;
}
.hero .btn:hover { border-color: #16171a; }
.hero .btn.solid { background: #16171a; border-color: #16171a; color: #ffffff; }
.hero .btn.solid:hover { background: #b8681e; border-color: #b8681e; }
.hero .scroll-cue { color: rgba(0, 0, 0, 0.4); }

/* The return leg of the loop, drawn rather than generated. */
.hero-loop {
  position: absolute; inset: 0; width: 100%; height: 100%;
  pointer-events: none; z-index: 2;
}
.hero-loop path {
  fill: none;
  stroke: #b8681e;
  stroke-width: 0.5;
  stroke-dasharray: 2.4 1.8;
  opacity: 0.85;
  vector-effect: non-scaling-stroke;
}
.hero-loop marker path { fill: #b8681e; stroke: none; }

/* Callouts over the artwork. Outline plus shadow, because the picture
   underneath is generated and its contrast is not guaranteed. */
.hero-marks { position: absolute; inset: 0; pointer-events: none; z-index: 3; }
.hm {
  position: absolute; transform: translate(-50%, -50%);
  display: grid; justify-items: center; gap: 1px;
  text-align: center; white-space: nowrap;
  paint-order: stroke;
  -webkit-text-stroke: 3.5px rgba(255, 255, 255, 0.95);
  text-shadow: 0 1px 0 rgba(255,255,255,0.9), 0 2px 12px rgba(0,0,0,0.3);
}
.hm b { font-size: 14.5px; font-weight: 700; letter-spacing: -0.015em; color: #16171a; }
.hm em { font-style: normal; font-size: 11px; color: #4a4f57; }
.hm.hinge b { color: #8a4a0d; }
.hm.on b { color: #145c3b; }

/* Below this the copy needs the whole frame, so the artwork steps back and
   the callouts go — four overlays cannot be legible at phone width. */
@media (max-width: 860px) {
  .hero-marks, .hero-loop { display: none; }
  .hero .hero-media::after {
    background: linear-gradient(180deg, rgba(255,255,255,0.72) 0%, rgba(255,255,255,0.95) 55%);
  }
}
"""
css_p.write_text(css, encoding="utf-8")
print("hero css added:", ".hero-loop" in css)
print("old hero.mp4 refs:", html.count("/img/hero.mp4"))
