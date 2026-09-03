"""Re-prompt the hero around a sortation conveyor instead of a robot arm.

Three complaints, one root cause: the machine was wrong.

A robotic arm moves ONE parcel at a time and has to reach across the line to
serve several lanes, which is why it looked physically impossible. It also
gives no reason for the output to be tidy, which is why the parcels stayed
chaotic after it.

A parcel sortation conveyor is the machine that actually does this job: a main
line carries the jumble past a row of diverters, each of which pushes an item
sideways into its own chute, and each chute feeds a lane where identical items
queue up evenly spaced. One messy input, several ordered outputs, no arm, and
every motion is something a real machine does.

That is also the fan-out that the earlier prompts kept failing to produce.
"""

from pathlib import Path

p = Path(__file__).resolve().parents[1] / "tools" / "gen_hero.py"
src = p.read_text(encoding="utf-8")

a = src.index("STILL = (")
b = src.index("def load_env()")

NEW = '''STILL = (
    "Wide panoramic isometric parcel sortation facility on a pure white floor, "
    "read left to right. "
    "LEFT, THE JUMBLE: one single input conveyor piled with a chaotic mess of "
    "cardboard parcels in many different sizes, tipped over at random angles, "
    "overlapping and crowding each other, a few tumbling off the sides onto the "
    "floor — visibly disordered and out of control. "
    "CENTRE, THE SORTER: that one line runs into a long orange sortation "
    "conveyor fitted with a row of sliding-shoe diverters along its side, each "
    "diverter pushing a parcel sideways off the main line. NO robotic arm, NO "
    "crane, NO gantry — only simple pushers and guide rails. "
    "RIGHT, THE LANES: four separate angled chutes lead down from those "
    "diverters into four parallel output lanes running side by side. In every "
    "lane the parcels are now IDENTICAL in size and perfectly evenly spaced in "
    "a straight neat queue, calm and regular. "
    "FAR RIGHT: a small desk with blank monitors where one figure stands. "
    "The contrast is the whole picture: one messy pile going in on the left, "
    "four tidy evenly spaced lanes coming out on the right. "
    "Wide clear margin along the top. " + STYLE + " "
    "--ar 16:9 --q 2 --s 160 --no robotic arm, robot arm, crane, gantry, claw, "
    "text, letters, numbers, words, signage, labels, captions, writing, watermark"
)

MOTION = (
    "Continuous fast motion. The input conveyor on the left carries the jumbled "
    "parcels along briskly. As each parcel reaches its diverter, the sliding "
    "shoe pushes it sideways off the main line and it slides down a chute into "
    "one of the four output lanes, one after another in quick succession. In "
    "the lanes the identical parcels move away steadily, evenly spaced. Nothing "
    "lifts, nothing flies, nothing is picked up — parcels only slide and are "
    "pushed. Quick industrial rhythm, everything running at once. Locked-off "
    "static camera, absolutely no camera movement, no zoom, no pan. " + STYLE
)

'''

src = src[:a] + NEW + src[b:]
p.write_text(src, encoding="utf-8")
print("sorter machine:", "sortation conveyor" in src)
print("arm excluded:", "--no robotic arm" in src)
print("lanes described:", "four parallel output lanes" in src)
print("physics constrained:", "nothing lifts" in src)
