"""Wire stage.css, drop the dead isometric styles, and start the factory video."""

from pathlib import Path

SITE = Path(__file__).resolve().parents[1] / "site"

# --- fix the typo that slipped into stage.css -------------------------------
css = (SITE / "stage.css").read_text(encoding="utf-8")
css = css.replace(".mk.on b { color: #14built; }\n", "")
(SITE / "stage.css").write_text(css, encoding="utf-8")
assert "built" not in css.split("Narrow screens")[0], "typo still present"

# --- link the new stylesheet ------------------------------------------------
html = (SITE / "index.html").read_text(encoding="utf-8")
if "stage.css" not in html:
    html = html.replace(
        '<link rel="stylesheet" href="/styles.css?v=21" />',
        '<link rel="stylesheet" href="/styles.css?v=21" />\n'
        '    <link rel="stylesheet" href="/stage.css?v=1" />',
    )

# --- start the factory video the same way the hero one starts ---------------
starter = """
    <script>
      // The factory animation is an upgrade on its own poster, never a
      // requirement: it only swaps in once it can actually play, and never for
      // reduced-motion. Same contract as the hero video.
      (function () {
        var v = document.querySelector(".iso-motion");
        if (!v || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
        var start = function () {
          v.src = "/img/factory.mp4";
          v.addEventListener("canplay", function () {
            v.classList.add("ready");
            v.play().catch(function () {});
          });
          v.load();
        };
        // Only fetch 8MB once the diagram is actually approaching the viewport.
        if ("IntersectionObserver" in window) {
          var io = new IntersectionObserver(function (es) {
            if (es.some(function (e) { return e.isIntersecting; })) {
              io.disconnect();
              start();
            }
          }, { rootMargin: "600px" });
          io.observe(v);
        } else {
          start();
        }
      })();
    </script>
  </body>"""

if "iso-motion" in html and 'v.src = "/img/factory.mp4"' not in html:
    html = html.replace("  </body>", starter, 1)

(SITE / "index.html").write_text(html, encoding="utf-8")
print("stage.css linked:", "stage.css" in html)
print("video starter:", 'factory.mp4' in html)
print("lazy via IO:", "rootMargin" in html)
