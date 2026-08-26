# -*- coding: utf-8 -*-
"""画面が使う作品台帳を書き出す(SPEC F-06)。

本文は索引が兼ねるので、ここに入れるのは表示と絞り込みに要る最小限だけ。
"""
import io, json, pathlib
ROOT = pathlib.Path(__file__).resolve().parent.parent
src = json.loads((ROOT / "data/works.json").read_text(encoding="utf-8"))["works"]
KANA = {"新字新仮名": 0, "新字旧仮名": 1, "旧字旧仮名": 2, "旧字新仮名": 3, "その他": 4}
NDC = {"NDC 913": "小説", "NDC 914": "随筆", "NDC 911": "詩歌", "NDC K913": "児童",
       "NDC 915": "紀行", "NDC 912": "戯曲", "NDC 933": "英米文学", "NDC 953": "仏文学",
       "NDC 943": "独文学", "NDC 983": "露文学", "NDC 289": "伝記", "NDC 910": "日本文学"}
out = {}
for w in src:
    out[int(w["id"])] = [w["title"], w["author"], KANA.get(w["kana"], 4),
                         NDC.get(w["ndc"], "その他"), w["chars"]]
dst = ROOT / "web/index/works.json"
dst.write_text(json.dumps({
    "fields": ["title", "author", "kana", "genre", "chars"],
    "kana_names": ["新字新仮名", "新字旧仮名", "旧字旧仮名", "旧字新仮名", "その他"],
    "works": out}, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
print(f"{len(out):,} 作 → {dst.relative_to(ROOT)} ({dst.stat().st_size / 1e3:.0f} KB)")
