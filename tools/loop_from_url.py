"""Pin a seamless loop to a still that is already reachable over HTTP.

The VOD upload API refused with UnsupportedRegion for this account, so instead
of fighting the region config we point the generator at the still we already
publish. The site is public HTTPS, and Wan takes `Type: "Url"` directly.

Same frame as FirstFrame and LastFrame, so the motion begins and ends exactly
where the poster does.

    python tools/loop_from_url.py https://herdup.sim3.app/img/hero-factory.png?v=3 site/img/hero-factory.mp4
"""

import json
import sys
import time
import urllib.request
from pathlib import Path

from tencentcloud.common import credential
from tencentcloud.common.profile.client_profile import ClientProfile
from tencentcloud.common.profile.http_profile import HttpProfile
from tencentcloud.vod.v20180717 import vod_client, models

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from gen_hero import MOTION, load_env  # noqa: E402


def main():
    if len(sys.argv) < 3:
        sys.exit("usage: loop_from_url.py <image-url> <out.mp4>")
    url, out = sys.argv[1], Path(sys.argv[2]).resolve()

    env = load_env()
    sub = int(env["TENCENT_SUB_APP_ID"])
    cred = credential.Credential(env["TENCENT_SECRET_ID"], env["TENCENT_SECRET_KEY"])
    http = HttpProfile(endpoint="vod.tencentcloudapi.com", reqTimeout=120)
    cli = vod_client.VodClient(cred, "ap-guangzhou", ClientProfile(httpProfile=http))

    print(f"[1/2] loop pinned to {url}", flush=True)
    req = models.CreateAigcVideoTaskRequest()
    req.from_json_string(
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
    task = json.loads(cli.CreateAigcVideoTask(req).to_json_string())["TaskId"]
    print(f"    task {task}", flush=True)

    detail, last, deadline = None, None, time.time() + 900
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
        sys.exit("finished, but no video url in the task detail")

    print("[2/2] downloading", flush=True)
    urllib.request.urlretrieve(vids[0], out)
    print(f"    saved {out} ({out.stat().st_size // 1024} KB)", flush=True)


if __name__ == "__main__":
    main()
