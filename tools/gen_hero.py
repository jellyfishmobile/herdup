"""Generate the hero: the whole solution story as one isometric factory.

The composition IS the argument, so it is specified zone by zone rather than
left to the model to infer:

    many chaotic sources  ->  deterministic declutter  ->  SEVERAL project
    lines running in parallel  ->  summary to the boss

The return leg (boss back to the client) is deliberately NOT asked of the
model. Generators fumble arrows and directionality; that arc is drawn as an
SVG overlay instead, where it is exact.

The loop is made with Wan's first/last-frame mode, passing the same frame as
both FirstFrame and LastFrame. Asking prose for "seamless loop" does not work;
pinning both ends does.

    python tools/gen_hero.py
"""

import json
import sys
import time
import urllib.request
from pathlib import Path

from tencentcloud.common import credential
from tencentcloud.common.exception.tencent_cloud_sdk_exception import (
    TencentCloudSDKException,
)
from tencentcloud.common.profile.client_profile import ClientProfile
from tencentcloud.common.profile.http_profile import HttpProfile
from tencentcloud.vod.v20180717 import vod_client, models

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "site" / "img"
REGION = "ap-guangzhou"

STYLE = (
    "flat isometric vector illustration, clean corporate infographic style, "
    "pure white seamless background, soft long shadows, amber orange and warm "
    "grey and slate blue palette, crisp flat colours, no gradients, "
    "uncluttered with generous empty white space between the groups of machines, "
    "ABSOLUTELY NO TEXT anywhere: no letters, no numbers, no words, no signage, "
    "no labels, no captions, no branding, no watermark; every machine panel and "
    "every screen is blank or shows only simple plain coloured bars and dots"
)

STILL = (
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
)

MOTION = (
    "Everything runs at once and continuously. Messy parcels keep tumbling in "
    "from the chutes on the left. The robotic arms of the central sorting "
    "machine lower and lift in a steady precise rhythm, one parcel after "
    "another. All three parallel conveyor lines run left to right at the same "
    "time, carrying evenly spaced identical boxes. Small worker figures make "
    "idle movements, machine indicator lights blink softly, the screens at the "
    "right-hand desk flicker gently. Locked-off static camera, absolutely no "
    "camera movement, no zoom, no pan. " + STYLE
)


def load_env() -> dict:
    env = {}
    for line in (ROOT / ".env").read_text(encoding="utf-8-sig").splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, v = line.split("=", 1)
            env[k.strip()] = v.strip().strip('"').strip("'")
    return env


def client(env):
    cred = credential.Credential(env["TENCENT_SECRET_ID"], env["TENCENT_SECRET_KEY"])
    http = HttpProfile(endpoint="vod.tencentcloudapi.com", reqTimeout=120)
    return vod_client.VodClient(cred, REGION, ClientProfile(httpProfile=http))


def poll(cli, sub_app_id, task_id, timeout=900):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        req = models.DescribeTaskDetailRequest()
        req.from_json_string(json.dumps({"SubAppId": sub_app_id, "TaskId": task_id}))
        d = json.loads(cli.DescribeTaskDetail(req).to_json_string())
        if d.get("Status") != last:
            print(f"    {d.get('Status')}", flush=True)
            last = d.get("Status")
        if d.get("Status") == "FINISH":
            return d
        time.sleep(10)
    raise TimeoutError(task_id)


def urls_from(detail):
    found = []

    def walk(n):
        if isinstance(n, dict):
            for k, v in n.items():
                if isinstance(v, str) and v.startswith("http") and "rl" in k:
                    found.append(v)
                else:
                    walk(v)
        elif isinstance(n, list):
            for i in n:
                walk(i)

    walk(detail)
    return list(dict.fromkeys(found))


def main():
    env = load_env()
    sub = int(env["TENCENT_SUB_APP_ID"])
    cli = client(env)
    OUT.mkdir(parents=True, exist_ok=True)

    print("[1/2] still — four zones, three parallel lines", flush=True)
    req = models.CreateAigcImageTaskRequest()
    req.from_json_string(
        json.dumps(
            {
                "SubAppId": sub,
                "ModelName": "MJ",
                "ModelVersion": "v7",
                "Prompt": STILL,
                "OutputConfig": {"StorageMode": "Permanent"},
            }
        )
    )
    task = json.loads(cli.CreateAigcImageTask(req).to_json_string())["TaskId"]
    print(f"    task {task}", flush=True)
    stills = urls_from(poll(cli, sub, task))
    if not stills:
        sys.exit("no still produced")

    # All four candidates are kept so the best composition can be chosen by eye;
    # the first is what the loop is pinned to, so poster and motion always match.
    for i, u in enumerate(stills[:4]):
        dest = OUT / (f"hero-factory{'' if i == 0 else f'-alt{i}'}.png")
        urllib.request.urlretrieve(u, dest)
        print(f"    saved {dest.name} ({dest.stat().st_size // 1024} KB)", flush=True)
    frame = stills[0]

    print("[2/2] loop — same frame pinned at both ends", flush=True)
    vreq = models.CreateAigcVideoTaskRequest()
    vreq.from_json_string(
        json.dumps(
            {
                "SubAppId": sub,
                "ModelName": "Wan",
                "ModelVersion": "3.0",
                "Prompt": MOTION,
                "FileInfos": [
                    {"Type": "Url", "Category": "Image", "Url": frame, "Usage": "FirstFrame"},
                    {"Type": "Url", "Category": "Image", "Url": frame, "Usage": "LastFrame"},
                ],
                "VideoOutputConfig": {
                    "StorageMode": "Permanent",
                    "Resolution": "1080P",
                    "AspectRatio": "16:9",
                    "Duration": 6,
                },
            }
        )
    )
    try:
        vtask = json.loads(cli.CreateAigcVideoTask(vreq).to_json_string())["TaskId"]
    except TencentCloudSDKException as e:
        sys.exit(f"video create failed: {e}")
    print(f"    task {vtask}", flush=True)
    vids = [u for u in urls_from(poll(cli, sub, vtask)) if ".mp4" in u.lower()]
    if not vids:
        sys.exit("no video produced")
    dest = OUT / "hero-factory.mp4"
    urllib.request.urlretrieve(vids[0], dest)
    print(f"    saved {dest.name} ({dest.stat().st_size // 1024} KB)", flush=True)


if __name__ == "__main__":
    main()
