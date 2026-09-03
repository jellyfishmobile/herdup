"""Make the factory line the hero.

The old hero was a dark photographic plate with white text. The new one is a
light flat-vector illustration, so the whole treatment inverts: dark text, a
white scrim on the reading side instead of a dark gradient, and the artwork
given room on the right rather than being covered.

The return leg of the loop — summary back to the client — is drawn as an SVG
overlay, not asked of the model: generators fumble arrows and directionality,
and that arc is the one part of the story the picture must not get wrong.
"""

from pathlib import Path

SITE = Path(__file__).resolve().parents[1] / "site"
html = (SITE / "index.html").read_text(encoding="utf-8")

start = html.index('    <section class="hero" id="top">')
end = html.index("    <!-- ============================================================\n         The story.")

NEW = '''    <section class="hero" id="top">
      <div class="hero-media">
        <img src="/img/hero-factory.png?v=1" alt="" />
        <video id="hero-video" muted loop playsinline preload="none"
               poster="/img/hero-factory.png?v=1" aria-hidden="true"></video>

        <!-- The return leg. The model draws the factory; this draws the one
             relationship it cannot be trusted with — that the summary goes
             back to the client, and the whole thing is a loop. -->
        <svg class="hero-loop" viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden="true">
          <defs>
            <marker id="loop-arrow" viewBox="0 0 10 10" refX="8" refY="5"
                    markerWidth="5" markerHeight="5" orient="auto-start-reverse">
              <path d="M 0 0 L 10 5 L 0 10 z" />
            </marker>
          </defs>
          <path d="M 86 30 C 86 39, 60 39, 40 39 C 22 39, 10 37, 8 30"
                marker-end="url(#loop-arrow)" />
        </svg>

        <div class="hero-marks" aria-hidden="true">
          <span class="hm" style="left:11%;top:44%"><b>Chaos in</b><em>every channel, any shape</em></span>
          <span class="hm hinge" style="left:44%;top:26%"><b>Sorted</b><em>the PM scopes the work</em></span>
          <span class="hm" style="left:64%;top:62%"><b>Run in parallel</b><em>several projects at once</em></span>
          <span class="hm on" style="left:88%;top:44%"><b>You get a summary</b><em>and reply to the client</em></span>
        </div>
      </div>

      <div class="hero-copy">
        <h1>The missing layer for vibe coders.</h1>
        <p class="lede">
          The AI coding CLIs are already good. What is missing is the layer above them &mdash;
          the one that turns whatever your client sends into work a team of agents actually
          does, and hands you back a summary.
        </p>
        <div class="cta">
          <a class="btn solid" href="#download">Download</a>
          <a class="btn" href="#roadmap">See the whole loop</a>
        </div>
        <p class="fineprint">
          Windows, macOS and Linux &middot; free and open source &middot; needs
          <a href="https://github.com/herdr-dev/herdr">herdr</a> installed
        </p>
      </div>
      <div class="scroll-cue" aria-hidden>Scroll</div>
    </section>

'''

html = html[:start] + NEW + html[end:]
html = html.replace('v.src = "/img/hero.mp4"', 'v.src = "/img/hero-factory.mp4"')
html = html.replace('<link rel="preload" as="image" href="/img/hero.png" />',
                    '<link rel="preload" as="image" href="/img/hero-factory.png?v=1" />')
html = html.replace('content="/img/hero.png"', 'content="/img/hero-factory.png"')
html = html.replace("styles.css?v=22", "styles.css?v=23")
html = html.replace("stage.css?v=1", "stage.css?v=2")

(SITE / "index.html").write_text(html, encoding="utf-8")
print("hero is the factory:", "hero-factory.png" in html)
print("loop arc overlay:", "hero-loop" in html)
print("hero labels:", html.count('class="hm'))
print("old hero refs left:", html.count("/img/hero.png") + html.count("/img/hero.mp4"))
