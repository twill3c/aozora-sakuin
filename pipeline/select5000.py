# -*- coding: utf-8 -*-
"""青空索引: 収録 5,000 作の選定(規則 v2)。

## v1 の何が問題だったか

v1 は「作品 ID 昇順に 1 著者 16 作まで」だった。決定論的で恣意も入らないが、
青空文庫の作品 ID は登録順(多作家では登録バッチ内の読み仮名順に近い)なので、
採られるのは機械的に先頭の 16 作になる。実測では代表作 21 点のうち採録は 11 点で、
芥川竜之介は羅生門も蜘蛛の糸も地獄変も入らず「十本の針」「あばばばば」が入っていた。
決定論と引き換えに代表性を失っていた。

## v2 の規則

順序に**公開された外部指標**を使う — 青空文庫自身のアクセスランキング
(2009-01 以降の月次・各 500 件、`data/ranking.json`)。私の趣味は入らず、
決定論も保たれる。

  1. 母集団 = 作品著作権フラグ「なし」× テキストファイルURL が .zip
  2. 第 1 周: ランキングスコア降順(同点は作品 ID 昇順)に、1 著者 16 作まで
  3. 第 2 周: ランキングに無い作品を作品 ID 昇順に、1 著者 16 作まで
  4. 第 3 周: 5,000 に足りない分を 17 作目から補充(同じ順序規則)
"""
import collections
import csv
import hashlib
import io
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CSV_IN = ROOT / "data/index_cache/list_person_all_extended_utf8.csv"
RANKING = ROOT / "data/ranking.json"
OUT = ROOT / "data/selection.tsv"
TARGET, CAP = 5000, 16
COLS = ["作品ID", "作品名", "姓", "名", "文字遣い種別", "分類番号", "テキストファイルURL", "順位スコア"]


def population():
    byid = {}
    for r in csv.DictReader(io.open(CSV_IN, encoding="utf-8-sig")):
        byid.setdefault(r["作品ID"], r)          # 共著・翻訳者による重複行は先頭を採る
    pop = [r for r in byid.values()
           if r.get("作品著作権フラグ") == "なし"
           and r.get("テキストファイルURL", "").endswith(".zip")]
    return sorted(pop, key=lambda r: int(r["作品ID"]))


def scores():
    if not RANKING.exists():
        raise SystemExit(f"{RANKING} がない。先に pipeline/fetch_ranking.py を実行する")
    d = json.loads(RANKING.read_text(encoding="utf-8"))
    return {w["id"]: w["score"] for w in d["works"]}


def select(pop, score):
    au = lambda r: r["姓"] + r["名"]
    key = lambda r: int(r["作品ID"])
    ranked = sorted(
        [r for r in pop if str(key(r)) in score],
        key=lambda r: (-score[str(key(r))], key(r)),
    )
    rest = [r for r in pop if str(key(r)) not in score]     # pop は既に ID 昇順

    c, sel, taken = collections.Counter(), [], set()

    def sweep(rows, cap):
        for r in rows:
            if len(sel) >= TARGET:
                return
            if r["作品ID"] in taken:
                continue
            if c[au(r)] < cap:
                c[au(r)] += 1
                sel.append(r)
                taken.add(r["作品ID"])

    sweep(ranked, CAP)          # 第 1 周: 読まれている作品から
    sweep(rest, CAP)            # 第 2 周: 残りを ID 昇順で
    sweep(ranked, CAP + 1)      # 第 3 周: 17 作目で 5,000 まで補充
    sweep(rest, CAP + 1)
    return sel, c


def write_tsv(sel, score, path):
    with io.open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\t".join(COLS) + "\n")
        for r in sorted(sel, key=lambda r: int(r["作品ID"])):
            sc = score.get(str(int(r["作品ID"])), 0)
            cells = [(r.get(k, "") or "").replace("\t", " ") for k in COLS[:-1]] + [str(sc)]
            f.write("\t".join(cells) + "\n")
    return hashlib.sha256(io.open(path, "rb").read()).hexdigest()


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else OUT
    pop = population()
    score = scores()
    sel, c = select(pop, score)
    digest = write_tsv(sel, score, out)
    ranked_in = sum(1 for r in sel if str(int(r["作品ID"])) in score)
    print(f"母集団 {len(pop):,} 作 → 選定 {len(sel):,} 作 / 著者 {len(c):,} 名")
    print(f"  うちランキング掲載作 {ranked_in:,} 作 / 掲載なし {len(sel) - ranked_in:,} 作")
    print(f"sha256 {digest}")
