import json
import pathlib
import sys
import time
import urllib.request

deadline = time.time() + 15
h = None
while time.time() < deadline:
    try:
        with urllib.request.urlopen("http://127.0.0.1:18600/health", timeout=1) as r:
            h = json.loads(r.read().decode())
        break
    except Exception:
        time.sleep(0.2)
if h is None:
    sys.exit("health unavailable")
pathlib.Path("tests/e2e/w14/reports/health_initial.json").write_text(
    json.dumps(h, ensure_ascii=False, indent=2), encoding="utf-8")
state = json.loads(pathlib.Path("tests/e2e/w14/run/state-A.json").read_text(encoding="utf-8"))
print(json.dumps(h, ensure_ascii=False))
print("INFOHASH=" + state["entries"][0]["infohash"])
