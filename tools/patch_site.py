"""Swap the SVG diagram for the AIGC factory video with an HTML label overlay.

Kept as a script rather than shell text-mangling: the block is HTML, and shell
here-strings kept eating the slashes.
"""

from pathlib import Path

SITE = Path(__file__).resolve().parents[1] / "site"
html = (SITE / "index.html").read_text(encoding="utf-8")

START = "        <!-- The whole solution as one isometric diagram"
END = "        <!-- The linear view, kept as the close-up of the middle of that loop. -->"

NEW = """        <!-- The generated factory runs underneath; the labels ride on top.
             AIGC supplies the look, HTML supplies the six captions -- models
             cannot be trusted with readable text and these have to be right.
             The poster is the still from the same prompt, so the fallback
             matches the motion. -->
        <figure id="iso-fig" class="iso-fig rise">
          <div class="iso-stage">
            <img class="iso-plate" src="/img/factory.png?v=2" alt="" />
            <video class="iso-motion" muted loop playsinline preload="none"
                   poster="/img/factory.png?v=2" aria-hidden="true"></video>

            <div class="iso-marks" aria-hidden="true">
              <span class="mk" style="left:9%;top:58%"><i></i><b>Your client</b><em>WhatsApp &middot; Telegram &middot; email</em></span>
              <span class="mk" style="left:26%;top:40%"><i></i><b>Client channel</b><em>one inbox, no AI</em></span>
              <span class="mk hinge" style="left:46%;top:22%"><i></i><b>herdupd + PM</b><em>chat becomes scoped work</em></span>
              <span class="mk on" style="left:68%;top:38%"><i></i><b>The team</b><em>coders &middot; QA &middot; shipping today</em></span>
              <span class="mk" style="left:84%;top:58%"><i></i><b>Board &amp; thread</b><em>kanban &middot; agent traffic</em></span>
              <span class="mk" style="left:60%;top:80%"><i></i><b>Executive summary</b><em>status &middot; KPIs &middot; back to you</em></span>
            </div>
          </div>

          <p class="sr-only">
            The loop: your client sends requests from a chat app; the client channel takes them in;
            herdupd and the PM turn them into scoped work; the team of coders and QA execute; the
            board and thread record it; the executive summary returns to your client.
          </p>

          <figcaption>
            Messy in, uniform out. What a client sends arrives whenever and in any shape &mdash; the
            PM is the hinge that turns it into work the team can actually execute, and what comes
            back to you is a summary. Only <strong>the team</strong> exists today; the launcher
            builds it in about twenty seconds.
          </figcaption>
        </figure>

"""

a = html.index(START)
b = html.index(END)
html = html[:a] + NEW + html[b:]

# The hand-drawn SVG is superseded.
html = html.replace('    <script src="/isometric.js?v=6"></script>\n', "")
html = html.replace('<script src="/isometric.js?v=6"></script>', "")
html = html.replace("v=19", "v=21")

(SITE / "index.html").write_text(html, encoding="utf-8")
print("iso-stage:", html.count("iso-stage"))
print("labels:", html.count('class="mk'))
print("isometric.js refs left:", html.count("isometric.js"))
