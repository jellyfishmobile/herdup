"""Generate landing-page imagery with Tencent VOD AIGC (CreateAigcImageTask).

Reads credentials from .env at the repo root; never prints them.

    python tools/gen_images.py hero
    python tools/gen_images.py --list

Design note on what we do and don't generate: anything that shows herdup itself
uses REAL screenshots captured from the built app (app/e2e/shots). Generated
imagery is only ever atmosphere, so the page never shows a picture of a product
that does not exist.
"""

import json
import os
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


def load_env() -> dict:
    env = {}
    path = ROOT / ".env"
    if not path.exists():
        sys.exit(f"no .env at {path}")
    for line in path.read_text(encoding="utf-8-sig").splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        env[k.strip()] = v.strip().strip('"').strip("'")
    for key in ("TENCENT_SECRET_ID", "TENCENT_SECRET_KEY", "TENCENT_SUB_APP_ID"):
        if not env.get(key):
            sys.exit(f"{key} missing from .env")
    return env


# The brand metaphor, taken seriously: herdup is named for herding. A warm dusk
# herding scene carries the name and the amber accent (--acc #b8681e), and is
# deliberately NOT the neon-cyberpunk default that every AI dev-tool page uses.
PROMPTS = {
    "hero": (
        "Wide cinematic landscape at golden hour, a lone figure on horseback guiding "
        "a herd across an open high plain, long raking sunlight, dust catching the "
        "light, deep amber and burnt sienna palette, muted slate blue sky, painterly "
        "realism, calm and purposeful mood, vast negative space in the upper third, "
        "no text, no logos "
        "--ar 21:9 --style raw --q 2 --s 250"
    ),
    "og": (
        "Cinematic golden hour scene, a rider guiding a small herd across an open "
        "plain, warm amber and burnt sienna light, muted slate sky, painterly "
        "realism, calm confident mood, generous empty sky for overlaid text, "
        "no text, no logos "
        "--ar 16:9 --style raw --q 2 --s 250"
    ),
    "texture": (
        "Extreme close up of weathered saddle leather, warm amber brown, soft raking "
        "light, fine grain and subtle scuffs, muted and desaturated, abstract, "
        "no text, no logos "
        "--ar 16:9 --style raw --q 2 --s 150"
    ),
}


# Wan 3.0 is the newest text-to-video model in the guide (2026.09.01) and
# supports 16:9 at 1080P. Kept short and slow-moving: this sits behind the
# headline, so motion must never compete with the text.
VIDEO_PROMPTS = {
    "hero": (
        "Slow cinematic drift across an open high plain at golden hour, a distant "
        "herd moving unhurried through low dust, long raking sunlight, deep amber "
        "and burnt sienna grass, muted slate blue sky above, painterly film grain, "
        "very slow camera push, calm and patient, no text, no logos, no people close up"
    ),
}


def create_video(cli, sub_app_id: int, prompt: str) -> str:
    req = models.CreateAigcVideoTaskRequest()
    req.from_json_string(
        json.dumps(
            {
                "SubAppId": sub_app_id,
                "ModelName": "Wan",
                "ModelVersion": "3.0",
                "Prompt": prompt,
                "VideoOutputConfig": {
                    "StorageMode": "Permanent",
                    "Resolution": "1080P",
                    "AspectRatio": "16:9",
                    "Duration": 5,
                },
            }
        )
    )
    resp = cli.CreateAigcVideoTask(req)
    return json.loads(resp.to_json_string())["TaskId"]


def client(env):
    cred = credential.Credential(env["TENCENT_SECRET_ID"], env["TENCENT_SECRET_KEY"])
    http = HttpProfile(endpoint="vod.tencentcloudapi.com", reqTimeout=120)
    prof = ClientProfile(httpProfile=http)
    return vod_client.VodClient(cred, REGION, prof)


def create(cli, sub_app_id: int, prompt: str) -> str:
    req = models.CreateAigcImageTaskRequest()
    req.from_json_string(
        json.dumps(
            {
                "SubAppId": sub_app_id,
                "ModelName": "MJ",
                "ModelVersion": "v7",
                "Prompt": prompt,
                "OutputConfig": {"StorageMode": "Permanent"},
            }
        )
    )
    resp = cli.CreateAigcImageTask(req)
    return json.loads(resp.to_json_string())["TaskId"]


def poll(cli, sub_app_id: int, task_id: str, timeout=600) -> dict:
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        req = models.DescribeTaskDetailRequest()
        req.from_json_string(json.dumps({"SubAppId": sub_app_id, "TaskId": task_id}))
        detail = json.loads(cli.DescribeTaskDetail(req).to_json_string())
        status = detail.get("Status")
        if status != last:
            print(f"    status: {status}")
            last = status
        if status == "FINISH":
            return detail
        time.sleep(10)
    raise TimeoutError(f"task {task_id} did not finish in {timeout}s")


def urls_from(detail: dict) -> list:
    """Pull image URLs out of the task detail without assuming one exact shape."""
    found = []

    def walk(node):
        if isinstance(node, dict):
            for k, v in node.items():
                if isinstance(v, str) and v.startswith("http") and (
                    "Url" in k or "url" in k
                ):
                    found.append(v)
                else:
                    walk(v)
        elif isinstance(node, list):
            for item in node:
                walk(item)

    walk(detail)
    # Keep order, drop duplicates.
    return list(dict.fromkeys(found))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("-")]
    if "--list" in sys.argv:
        print("\n".join(PROMPTS))
        return
    wanted = args or ["hero"]

    env = load_env()
    sub_app_id = int(env["TENCENT_SUB_APP_ID"])
    cli = client(env)
    OUT.mkdir(parents=True, exist_ok=True)

    video = "--video" in sys.argv
    table = VIDEO_PROMPTS if video else PROMPTS

    for name in wanted:
        if name not in table:
            print(f"!! no prompt named {name!r}; try --list")
            continue
        print(f"\n[{name}] creating {'video' if video else 'image'} task…")
        try:
            task_id = (
                create_video(cli, sub_app_id, table[name])
                if video
                else create(cli, sub_app_id, table[name])
            )
        except TencentCloudSDKException as e:
            print(f"!! create failed: {e}")
            continue
        print(f"    task {task_id}")
        try:
            detail = poll(cli, sub_app_id, task_id)
        except (TencentCloudSDKException, TimeoutError) as e:
            print(f"!! poll failed: {e}")
            continue

        (OUT / f"{name}.task.json").write_text(
            json.dumps(detail, indent=1, ensure_ascii=False), encoding="utf-8"
        )
        urls = urls_from(detail)
        if not urls:
            print("!! finished but no image URL found; see the saved task json")
            continue
        for i, url in enumerate(urls):
            low = url.lower()
            ext = ".mp4" if ".mp4" in low else ".png" if ".png" in low else ".jpg"
            dest = OUT / (f"{name}{ext}" if i == 0 else f"{name}-{i}{ext}")
            urllib.request.urlretrieve(url, dest)
            print(f"    saved {dest.relative_to(ROOT)} ({dest.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()
