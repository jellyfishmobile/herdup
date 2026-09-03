"""Put the positioning on the page: the missing layer, and who it is for.

The sharpest articulation of the product so far, in the author's words: the
plumbing already exists for AI CLIs, but nothing links it together
automatically -- and the relationship that actually needs fixing is the one
between a client and a small studio. Automate it, de-chaos it, get it done.
"""

from pathlib import Path

SITE = Path(__file__).resolve().parents[1] / "site"
html = (SITE / "index.html").read_text(encoding="utf-8")


def swap(old: str, new: str, label: str) -> None:
    global html
    if old not in html:
        raise SystemExit(f"anchor missing: {label}")
    html = html.replace(old, new, 1)


# The best one-line description of the product now exists, so use it where a
# one-liner is all you get.
swap(
    'content="One agent is a better autocomplete. A team is a different thing — but the setup cost stops most people finding out. herdup removes the setup cost."',
    'content="The plumbing for AI coding agents is already here. The layer that links it together is not. herdup is that layer: automate the setup, de-chaos the client work, get things done."',
    "og:description",
)

swap(
    'content="A desktop launcher for herdr. Pick a project, pick a team, and herdup starts several AI coding agents side by side in one terminal — in about twenty seconds."',
    'content="The missing layer for vibe coders and small studios: herdup links your AI coding CLIs into one working team, and turns messy client requests into work that actually gets done."',
    "meta description",
)

# The category claim belongs before the attention argument: it says what herdup
# IS, and the attention point says why it matters.
swap(
    '      <p class="eyebrow rise">The argument</p>\n'
    '      <h2 class="rise big">Your scarce resource is attention, not typing.</h2>\n',
    '      <p class="eyebrow rise">The argument</p>\n'
    '      <h2 class="rise big">The plumbing is already here. The layer isn&rsquo;t.</h2>\n'
    "\n"
    '      <p class="rise lead-para">\n'
    "        Claude Code, Codex, Gemini, Cursor, Copilot &mdash; the agents are good and getting\n"
    "        better every month. That part is solved. What is missing is the layer above them: the\n"
    "        thing that links them into a team automatically, so one person can direct four agents\n"
    "        instead of babysitting four terminals. Everyone is shipping better plumbing. Nobody is\n"
    "        shipping the layer.\n"
    "      </p>\n"
    '      <p class="rise">\n'
    "        And there is a second gap, further out. The way work passes between a client and a small\n"
    "        studio has barely changed: a message arrives on WhatsApp, someone translates it into\n"
    "        tasks, someone reports back. That translation is manual, it is the bottleneck, and it is\n"
    "        exactly the kind of work these agents are now capable of. herdup&rsquo;s job is to\n"
    "        automate it, take the chaos out of it, and get it done.\n"
    "      </p>\n"
    "\n"
    '      <h3 class="rise big sub-thesis">Your scarce resource is attention, not typing.</h3>\n',
    "argument opening",
)

(SITE / "index.html").write_text(html, encoding="utf-8")
print("missing-layer headline:", "The plumbing is already here" in html)
print("soho paragraph:", "small\n        studio" in html or "small studio" in html)
print("attention kept:", "attention, not typing" in html)
