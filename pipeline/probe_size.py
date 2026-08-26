# -*- coding: utf-8 -*-
"""5,000 作規模の索引サイズを確定するための無作為 100 作サンプル取得。
母集団 = 青空文庫 著作権なし × txt zip あり × 1著者16作上限（4,925 作）。
礼儀: 0.8 秒間隔・連絡先入り UA・一度きり。"""
import collections, csv, io, json, pathlib, random, re, time, urllib.request, zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
CSV  = ROOT / "data/index_cache/list_person_all_extended_utf8.csv"
OUT  = ROOT / "data/probe.json"
UA  = "aozora-sakuin sizing probe (personal research; contact: twill3c@gmail.com)"
INTERVAL, N, SEED, CAP = 0.8, 100, 20260826, 16

rows = list(csv.DictReader(io.open(CSV, encoding="utf-8-sig")))
byid = {}
for r in rows:
    byid.setdefault(r["作品ID"], r)
pop = [r for r in byid.values()
       if r.get("作品著作権フラグ") == "なし"
       and r.get("テキストファイルURL", "").endswith(".zip")]
pop.sort(key=lambda r: r["作品ID"])

c, sel = collections.Counter(), []
for r in pop:
    a = r["姓"] + r["名"]
    if c[a] < CAP:
        c[a] += 1
        sel.append(r)
print(f"母集団 {len(pop):,} → 上限{CAP}作の選定 {len(sel):,} 作から {N} 作を無作為抽出（seed={SEED}）", flush=True)

rng = random.Random(SEED)
sample = rng.sample(sel, N)

HDR  = re.compile(r"^.*?-{20,}\s*\n.*?-{20,}\s*\n", re.S)
FTR  = re.compile(r"\n底本[：:].*\Z", re.S)
RUBY = re.compile(r"《[^》]*》")
ANNO = re.compile(r"［＃[^］]*］")

def normalize(t):
    t = HDR.sub("", t, count=1)
    t = FTR.sub("", t, count=1)
    t = ANNO.sub("", RUBY.sub("", t)).replace("｜", "")
    return re.sub(r"[ \t\u3000]+", "", t)

recs, fails = [], []
for i, r in enumerate(sample, 1):
    url = r["テキストファイルURL"]
    try:
        req = urllib.request.Request(url, headers={"User-Agent": UA})
        blob = urllib.request.urlopen(req, timeout=45).read()
        with zipfile.ZipFile(io.BytesIO(blob)) as z:
            name = next(x for x in z.namelist() if x.lower().endswith(".txt"))
            raw = z.read(name)
        try:
            t = raw.decode("cp932")
        except UnicodeDecodeError:
            t = raw.decode("utf-8", errors="replace")
        body = normalize(t)
        recs.append({
            "id": r["作品ID"], "title": r["作品名"], "author": r["姓"] + r["名"],
            "kana": r.get("文字遣い種別", ""), "ndc": (r.get("分類番号", "") or "").split("/")[0].strip(),
            "zip": len(blob), "raw_chars": len(t), "body_chars": len(body),
            "body_utf8": len(body.encode("utf-8")),
        })
    except Exception as e:
        fails.append({"id": r["作品ID"], "title": r["作品名"], "err": f"{type(e).__name__}: {e}"})
    if i % 20 == 0:
        print(f"  {i}/{N} 取得（失敗 {len(fails)}）", flush=True)
    time.sleep(INTERVAL)

json.dump({"seed": SEED, "cap": CAP, "pop": len(pop), "selected": len(sel),
           "n": N, "records": recs, "fails": fails},
          io.open(OUT, "w", encoding="utf-8"), ensure_ascii=False)
print(f"完了: 成功 {len(recs)} / 失敗 {len(fails)} → {OUT}")
