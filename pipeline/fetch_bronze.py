# -*- coding: utf-8 -*-
"""青空索引: 選定 5,000 作の本文を取得する(SPEC F-02 / N-03)。

規範:
  - リクエスト間隔 INTERVAL 秒以上(既定 0.8)
  - 連絡先入り User-Agent
  - レジューム可 — data/raw/{作品ID}.txt が既にあれば取得しない
  - 取得したものは cp932 から復号して UTF-8 で保存する(正規化は normalize.py の役目)
"""
import csv
import io
import json
import pathlib
import sys
import time
import urllib.error
import urllib.request
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
SELECTION = ROOT / "data/selection.tsv"
RAW = ROOT / "data/raw"
REPORT = ROOT / "data/fetch_report.json"
UA = "aozora-sakuin corpus builder (personal research; contact: twill3c@gmail.com)"
INTERVAL = 0.8
RETRIES = 2


def fetch_one(url):
    last = None
    for attempt in range(RETRIES + 1):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=60) as res:
                blob = res.read()
            with zipfile.ZipFile(io.BytesIO(blob)) as z:
                name = next(n for n in z.namelist() if n.lower().endswith(".txt"))
                data = z.read(name)
            try:
                return data.decode("cp932"), len(blob)
            except UnicodeDecodeError:
                return data.decode("utf-8", errors="replace"), len(blob)
        except Exception as e:                      # noqa: BLE001 — 全失敗を記録対象にする
            last = f"{type(e).__name__}: {e}"
            if attempt < RETRIES:
                time.sleep(INTERVAL * (attempt + 2))
    raise RuntimeError(last)


def main():
    RAW.mkdir(parents=True, exist_ok=True)
    with io.open(SELECTION, encoding="utf-8") as f:
        rows = list(csv.DictReader(f, delimiter="\t"))

    todo = [r for r in rows if not (RAW / f"{r['作品ID']}.txt").exists()]
    print(f"選定 {len(rows):,} 作 / 取得済み {len(rows) - len(todo):,} 作 / 今回取得 {len(todo):,} 作", flush=True)
    print(f"間隔 {INTERVAL}s → 所要見込み 約 {len(todo) * INTERVAL / 60:.0f} 分", flush=True)

    ok, fails, bytes_in, t0 = 0, [], 0, time.time()
    for i, r in enumerate(todo, 1):
        try:
            text, zsize = fetch_one(r["テキストファイルURL"])
            (RAW / f"{r['作品ID']}.txt").write_text(text, encoding="utf-8", newline="\n")
            ok += 1
            bytes_in += zsize
        except Exception as e:                      # noqa: BLE001
            fails.append({"id": r["作品ID"], "title": r["作品名"],
                          "url": r["テキストファイルURL"], "err": str(e)})
        if i % 250 == 0 or i == len(todo):
            el = time.time() - t0
            rate = i / el if el else 0
            print(f"  {i:,}/{len(todo):,}  成功 {ok:,} 失敗 {len(fails)}  "
                  f"経過 {el/60:.1f}分  残り 約 {(len(todo)-i)/rate/60 if rate else 0:.1f}分", flush=True)
        time.sleep(INTERVAL)

    total = sum(f.stat().st_size for f in RAW.glob("*.txt"))
    REPORT.write_text(json.dumps(
        {"selected": len(rows), "attempted": len(todo), "ok": ok,
         "failed": len(fails), "fails": fails,
         "raw_files": len(list(RAW.glob("*.txt"))), "raw_bytes": total,
         "elapsed_sec": round(time.time() - t0, 1)},
        ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"\n完了: 成功 {ok:,} / 失敗 {len(fails)} / data/raw に {len(list(RAW.glob('*.txt'))):,} 作"
          f" ({total/1e6:.0f} MB) → {REPORT}", flush=True)
    return 1 if len(fails) > len(todo) * 0.02 else 0    # 失敗 2% 超で異常終了


if __name__ == "__main__":
    sys.exit(main())
