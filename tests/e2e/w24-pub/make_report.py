"""W24-pub / FR-24 判定：S2 发布证据 vs 生产 coldrevive-ed2k 检索结果。"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
OUT = ROOT / "reports"
checks = []


def check(name, ok, detail):
    checks.append((name, bool(ok), detail))


def load(name):
    return json.loads((OUT / name).read_text(encoding="utf-8"))


pub = load("publish-after-restart.json")
res = load("search-after-publish.json")
term = "fr24-after-restart"

check("s2_publish_confirmed", pub["verdict"] == "confirmed_unnamespaced_keyword_publish_then_retrieve"
      and pub["publish_acked"] and pub["our_entry_returned"], pub["verdict"])
check("same_term", pub["keyword"] == term and res["term"] == term, term)
check("key_matches_s2", pub["keyword_key"] == res["keyword_key"],
      f"s2={pub['keyword_key']} prod={res['keyword_key']}")
check("key_is_unnamespaced_md4", res["keyword_key_is_unnamespaced_md4"] is True
      and pub["keyword_key_is_namespaced"] is False, res["keyword_key"])
check("kad_node_echoes_target", res["contacts"][0]["target_matches_key"] is True,
      res["contacts"][0])
check("bootstrap_rtt_is_real", res["contacts"][0]["bootstrap_ms"] > 0
      and res["contacts"][0]["rtt_ms"] > 0, res["contacts"][0])
check("answers_returned", res["answer_count"] >= 1, str(res["answer_count"]))
check("file_id_matches_published_hash", res["answers"][0]["file_id"] == pub["ed2k_hash"],
      f"{res['answers'][0]['file_id']} vs {pub['ed2k_hash']}")
check("ed2k_link_matches_published", res["ed2k_links"][0] == pub["ed2k_link"],
      pub["ed2k_link"])
check("kad_tags_decoded", len(res["answers"][0]["tags"]) >= 2
      and any(t["type"] == 2 for t in res["answers"][0]["tags"]),
      res["answers"][0]["tags"])
check("evidence_flag_not_faked", res["evidence_valid_for_pass"] is True,
      "只在拿到可解析 answer + ed2k link 时为 true")

direct = load("public-search.json")
check("direct_public_attempt_boundary_recorded", direct["answer_count"] == 0
      and direct["public_kad_contacted"] is True,
      "A 主机直连公共节点未拿到结果；本次通过口径是 221 aMuleD 这个真实 KAD 节点")

passed = sum(1 for _, ok, _ in checks if ok)
report = {
    "schema": "coldrevive.w24.fr24-public-track/v1",
    "scope": "真实 aMuleD KAD 节点上先确认关键词条目存在，再用生产 coldrevive-ed2k "
             "发 BOOTSTRAP_REQ + 0x33 SEARCH_KEY_REQ 独立读回并生成标准 ed2k:// 链接",
    "checks": [{"name": n, "ok": ok, "detail": d} for n, ok, d in checks],
    "passed": passed,
    "total": len(checks),
    "boundary": "查询目标是 192.168.1.221:4672 的 aMuleD（它自身连着公共 KAD）；"
                "A 主机直连公网节点未拿到结果，已保留 public-search.json。"
                "本证据不宣称下载可用（FR-25 另测），不宣称 credit / 上传队列完成。"
                "g2_claim=false、s6_status=not_run 不变",
}
(OUT / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
for n, ok, d in checks:
    print(f"[{'OK' if ok else 'XX'}] {n}: {d}")
print(f"passed={passed}/{len(checks)}")
raise SystemExit(0 if passed == len(checks) else 1)
