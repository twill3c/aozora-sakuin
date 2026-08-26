# -*- coding: utf-8 -*-
"""青空文庫のアクセスランキング(月次・各 500 件)を取得する(SPEC F-01 / N-03)。

収録作を選ぶ順序に、私の趣味ではなく**公開された外部指標**を使うためのもの。
2009-01 以降の月次ランキングを全て集計し、作品 ID ごとの順位スコアを出す。

スコア = Σ (501 - その月の順位)。上位に長く居続けた作品ほど高い。
"""
import io
import json
import pathlib
import re
import time
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "data/ranking.json"
BASE = "https://www.aozora.gr.jp/access_ranking/"
UA = "aozora-sakuin corpus builder (personal research; contact: twill3c@gmail.com)"
INTERVAL = 0.8
PER_PAGE = 500

MONTH = re.compile(r'href="(\d{4}_\d{2}_txt\.html)"')
ENTRY = re.compile(r'card(\d+)\.html"[^>]*>\s*([^<]+?)\s*<')


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    blob = urllib.request.urlopen(req, timeout=45).read()
    try:
        return blob.decode("utf-8")
    except UnicodeDecodeError:
        return blob.decode("cp932", errors="replace")


def main():
    index = get(BASE)
    months = sorted(set(MONTH.findall(index)))
    print(f"月次ランキング {len(months)} 本({months[0]} 〜 {months[-1]})", flush=True)

    scores, appear, best, titles = {}, {}, {}, {}
    for i, m in enumerate(months, 1):
        try:
            html = get(BASE + m)
        except Exception as e:  # noqa: BLE001
            print(f"  {m} 取得失敗 {type(e).__name__}: {e}", flush=True)
            time.sleep(INTERVAL)
            continue
        rows = ENTRY.findall(html)
        for rank, (cid, title) in enumerate(rows, 1):
            wid = str(int(cid))
            scores[wid] = scores.get(wid, 0) + (PER_PAGE + 1 - rank)
            appear[wid] = appear.get(wid, 0) + 1
            best[wid] = min(best.get(wid, 10**9), rank)
            titles.setdefault(wid, title)
        if i % 40 == 0 or i == len(months):
            print(f"  {i}/{len(months)} 集計(異なり {len(scores):,} 作)", flush=True)
        time.sleep(INTERVAL)

    ranked = sorted(scores.items(), key=lambda kv: (-kv[1], int(kv[0])))
    OUT.write_text(
        json.dumps(
            {
                "months": months,
                "per_page": PER_PAGE,
                "distinct": len(scores),
                "works": [
                    {
                        "id": wid,
                        "score": sc,
                        "months": appear[wid],
                        "best_rank": best[wid],
                        "title": titles[wid],
                    }
                    for wid, sc in ranked
                ],
            },
            ensure_ascii=False,
        ),
        encoding="utf-8",
    )
    print(f"\n異なり {len(scores):,} 作 → {OUT}")
    print("上位 10:")
    for w in ranked[:10]:
        wid = w[0]
        print(f"  {wid:>6} {titles[wid][:24]:24s} score {w[1]:,} / {appear[wid]} か月 / 最高 {best[wid]} 位")


if __name__ == "__main__":
    main()
