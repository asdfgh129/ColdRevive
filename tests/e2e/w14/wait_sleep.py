import json
import pathlib
import sys
import time
import urllib.request

for _ in range(30):
    with urllib.request.urlopen("http://127.0.0.1:18600/health", timeout=2) as r:
        h = json.loads(r.read().decode())
    if h["idle"]["sleeps"] >= 1:
        break
    time.sleep(0.25)
pathlib.Path("tests/e2e/w14/reports/health_after_sleep.json").write_text(
    json.dumps(h, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(h, ensure_ascii=False))
sys.exit(0 if h["idle"]["sleeps"] >= 1 and h["seed_active"] == 0 else 1)
