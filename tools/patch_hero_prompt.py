"""Re-prompt the hero: visible chaos, a real diverter, and speed.

Three specific failures in the last render, all of them prompt problems:

* "chaos" was described as parcels "piling up messily", which the model drew as
  a tidy warehouse stack. Chaos has to be described as motion and disorder:
  falling, tumbling, spilling on the floor, tipped at angles.
* "three conveyor lines running parallel" gave three unrelated belts with no
  mechanism. The real story is ONE inbound belt hitting a sorter that DIVERTS
  parcels onto several outgoing belts — the fan-out is the point, and it is
  what makes "several projects at once" legible.
* The motion was slow because nothing asked for speed.
"""

from pathlib import Path

p = Path(__file__).resolve().parents[1] / "tools" / "gen_hero.py"
src = p.read_text(encoding="utf-8")

a = src.index("STILL = (")
b = src.index("MOTION = (")
c = src.index("def load_env()")

STILL = '''STILL = (
    "Wide panoramic isometric factory floor, one continuous scene read left to "
    "right on a pure white floor. "
    "FAR LEFT, VISIBLE DISORDER: cardboard parcels of wildly different sizes "
    "raining down out of four separate chutes at once, caught mid-fall, "
    "bouncing and tumbling, several boxes lying tipped over on their sides and "
    "scattered loose across the floor at random angles, a lopsided leaning "
    "heap spilling outward — obviously messy and out of control. "
    "FROM THAT MESS: one single wide conveyor belt gathers it all up and "
    "carries it toward the centre. "
    "CENTRE, THE SORTER: one large orange machine straddling that belt with a "
    "pusher arm, and FOUR narrower conveyor belts fanning outward from it "
    "diagonally to the right at different angles, each one receiving neat "
    "identical boxes being pushed onto it — the single messy stream visibly "
    "splitting into four tidy streams. "
    "RIGHT: the four belts run past small workstations with worker figures to "
    "one raised desk with blank monitors where a single figure stands. "
    "Unmistakable contrast: falling, scattered and random on the left; four "
    "orderly evenly spaced streams on the right. "
    "Wide clear margin along the top. " + STYLE + " "
    "--ar 16:9 --q 2 --s 160 --no text, letters, numbers, words, signage, labels, "
    "captions, writing, typography, watermark"
)

'''

MOTION = '''MOTION = (
    "Fast, busy, continuous motion. Parcels rain down rapidly from the chutes "
    "on the left and tumble onto the belt. The single inbound conveyor moves "
    "briskly. The sorter's pusher arm works quickly, flicking each box onto one "
    "of the four outgoing belts in rapid succession. All four belts run fast, "
    "carrying evenly spaced boxes away to the right at a steady clip. Machine "
    "lights blink, workers move. Energetic industrial rhythm, quick cadence, "
    "everything moving at once. Locked-off static camera, absolutely no camera "
    "movement, no zoom, no pan. " + STYLE
)

'''

src = src[:a] + STILL + MOTION + src[c:]
p.write_text(src, encoding="utf-8")
print("diverter in prompt:", "fanning outward" in src)
print("chaos as motion:", "raining down" in src)
print("speed asked for:", "Fast, busy" in src)
