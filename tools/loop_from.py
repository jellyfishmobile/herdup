"""Make a seamless loop from a chosen still.

MJ returns four candidates and the best composition is rarely the first, but
the video generator needs a URL, not a local file — so previously the loop was
always pinned to candidate #1 whether or not that was the good one.

This uploads the still you picked to VOD, then generates the loop with that
frame pinned as BOTH FirstFrame and LastFrame, so the motion starts and ends
exactly where the poster does.

    python tools/loop_from.py site/img/hero-factory.png
"""

import json
import sys
import time
import urllib.request
from pathlib import Path

from qcloud_vod.model import VodUploadRequest
from qcloud_vod.vod_upload_client import VodUploadClient
from tencentcloud.common import credential
from tencentcloud.common.profile.client_profile import ClientProfile
from tencentcloud.common.profile.http_profile import HttpProfile
from tencentcloud.vod.v20180717 import vod_client, models

ROOT = Path(__file__).resolve().parents[1]
REGION = "ap-guangzhou"

sys.path.insert(0, str(ROOT / "tools"))
from gen_hero import MOTION, load_env  # noqa: E402  (shares one motion prompt)


def main():
    if len(sys.argv) < 2:
        sys.exit("usage: loop_from.py <image> [out.mp4]")
    img = Path(sys.argv[1]).resolve()
    if not img.exists():
        sys.exit(f"no such file: {img}")
    out = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else img.with_suffix(".mp4")

    env = load_env()
    sub = int(env["TENCENT_SUB_APP_ID"])

    print(f"[1/3] uploading {img.name}")
    up = VodUploadClient(env["TENCENT_SECRET_ID"], env["TENCENT_SECRET_KEY"])
    req = VodUploadRequest()
    req.SubAppId = sub
    req.MediaFilePath = str(img)
    resp = up.upload(REGION, req)
    url = resp.MediaUrl
    print(f"    {url}")

    cred = credential.Credential(env["TENCENT_SECRET_ID"], env["TENCENT_SECRET_KEY"])
    http = HttpProfile(endpoint="vod.tencentcloudapi.com", reqTimeout=120)
    cli = vod_client.VodClient(cred, REGION, ClientProfile(httpProfile=http))

    print("[2/3] generating the loop, pinned to that exact frame")
    vreq = models.CreateAigcVideoTaskRequest()
    vreq.from_json_string(
        json.dumps(
            {
                "SubAppId": sub,
                "ModelName": "Wan",
                "ModelVersion": "3.0",
                "Prompt": MOTION,
                "FileInfos": [
                    {"Type": "Url", "Category": "Image", "Url": url, "Usage": "FirstFrame"},
                    {"Type": "Url", "Category": "Image", "Url": url, "Usage": "LastFrame"},
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
    task = json.loads(cli.CreateAigcVideoTask(vreq).to_json_string())["TaskId"]
    print(f"    task {task}")

    deadline = time.time() + 900
    last = None
    detail = None
    while time.time() < deadline:
        q = models.DescribeTaskDetailRequest()
        q.from_json_string(json.dumps({"SubAppId": sub, "TaskId": task}))
        detail = json.loads(cli.DescribeTaskDetail(q).to_json_string())
        if detail.get("Status") != last:
            print(f"    {detail.get('Status')}", flush=True)
            last = detail.get("Status")
        if detail.get("Status") == "FINISH":
            break
        time.sleep(10)
    else:
        sys.exit("timed out")

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
    vids = [u for u in dict.fromkeys(found) if ".mp4" in u.lower()]
    if not vids:
        sys.exit("no video in the finished task")

    print("[3/3] downloading")
    urllib.request.urlretrieve(vids[0], out)
    print(f"    saved {out} ({out.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()
