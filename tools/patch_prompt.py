"""Rewrite the hero prompt so the model stops drawing garbled lettering.

Two causes in the previous attempt, both mine:

* Saying "ZONE 1 / ZONE 2 / ZONE 3" made the model render zone *signage* — it
  read the structure as something to label. Describe position spatially and
  never use the word.
* "screens showing charts" invites text on every display. Ask for blank panels
  and plain coloured bars instead.

Diffusion models cannot spell. Any wording that implies a sign, a label, a
readout or a caption will come back as convincing gibberish, and gibberish on
a hero reads as unfinished. All captions are HTML overlays instead.
"""

from pathlib import Path

p = Path(__file__).resolve().parents[1] / "tools" / "gen_hero.py"
src = p.read_text(encoding="utf-8")

STYLE_OLD = '''STYLE = (
    "flat isometric vector illustration, clean corporate infographic style, "
    "pure white background, soft long shadows, amber orange and teal and slate "
    "blue palette, crisp flat colours, no gradients, no text, no logos, no watermark"
)'''
STYLE_NEW = '''STYLE = (
    "flat isometric vector illustration, clean corporate infographic style, "
    "pure white seamless background, soft long shadows, amber orange and warm "
    "grey and slate blue palette, crisp flat colours, no gradients, "
    "uncluttered with generous empty white space between the groups of machines, "
    "ABSOLUTELY NO TEXT anywhere: no letters, no numbers, no words, no signage, "
    "no labels, no captions, no branding, no watermark; every machine panel and "
    "every screen is blank or shows only simple plain coloured bars and dots"
)'''

STILL_OLD_START = 'STILL = ('
STILL_NEW = '''STILL = (
    "Wide panoramic isometric factory floor, one continuous scene read from "
    "left to right, with clear empty white space separating each group. "
    "FAR LEFT: five separate chutes angled in from different directions, each "
    "pouring a stream of cardboard parcels of many different sizes and "
    "rotations, tumbling and overlapping and crooked, piling up messily at the "
    "start of the line — visibly disordered. "
    "LEFT OF CENTRE: one large orange machine with two robotic arms reaching "
    "down onto the belt, taking the jumbled parcels in on one side and putting "
    "out identical uniform boxes on the other, one small worker figure beside it. "
    "CENTRE AND RIGHT: three separate conveyor lines running parallel to each "
    "other, one behind another, all running at the same time, each carrying "
    "identical evenly spaced boxes past its own small machines and worker "
    "figures. "
    "FAR RIGHT: a raised desk where one single figure stands at a bank of "
    "blank monitors, with tidy stacks of finished boxes beside it. "
    "The change is obvious at a glance: random and piled on the left, uniform "
    "and evenly spaced on the right. "
    "Wide clear margin along the top of the image. " + STYLE + " "
    "--ar 16:9 --q 2 --s 160 --no text, letters, numbers, words, signage, labels, "
    "captions, writing, typography, watermark"
)'''

a = src.index(STILL_OLD_START)
b = src.index("MOTION = (")
src = src[:a] + STILL_NEW + "\n\n" + src[b:]
src = src.replace(STYLE_OLD, STYLE_NEW)

# The motion prompt must not reintroduce text either.
src = src.replace(
    "the screens at the "
    '    "right-hand desk flicker gently. Locked-off static camera, absolutely no "',
    "the blank screens at the "
    '    "right-hand desk glow gently. Locked-off static camera, absolutely no "',
)

p.write_text(src, encoding="utf-8")
print("no-text negative added:", "--no text" in src)
print("zone wording gone:", "ZONE" not in src)
