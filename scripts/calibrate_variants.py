# -*- coding: utf-8 -*-
"""表記ゆれ規則の較正(SPEC F-07 / O-17)。

  python scripts/calibrate_variants.py

各規則について「その異体形の出現が、旧仮名の作品にどれだけ落ちるか」を全 5,000 作で測る。
青空文庫の台帳が各作品に付けている**文字遣い種別**は、本文とは独立した外部ラベルなので、
これで測れば「規則が本物の仮名遣いを捉えているか」を循環なしに判定できる。

割合が高ければ、その異体形は旧仮名の作品にしか現れない = 本物の仮名遣い。
低ければ、助詞の境界などとの偶然の衝突である(「をもう」が「…を、もう…」に当たる等)。

出力: data/variant_calibration.json — Rust の規則表(`rust/src/variants.rs`)に
書いてある old_ratio は、この実測値と一致していなければならない(ゲート V-01)。
"""
import collections
import glob
import io
import json
import os
import pathlib
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "data/variant_calibration.json"
OLD_KANA = ("新字旧仮名", "旧字旧仮名")

# 規則 → 較正に使う異体形。実際に本文に現れる語で測る
PROBES = {
    "い→ゐ": ["ゐる", "ゐない", "つひに"],
    "え→ゑ": ["こゑ", "すゑ"],
    "じ→ぢ": ["すぢ", "もみぢ", "はぢめ"],
    "ず→づ": ["しづか", "みづ", "つづみ"],
    "う→ふ(語中)": ["いふ", "おもふ", "さうらふ"],
    "お→ほ(語中)": ["なほ", "こほり", "とほい"],
    "わ→は(語中)": ["かはいい", "あはれ", "かはり"],
    "長音 こう→かう": ["かうして", "かういふ"],
    "長音 そう→さう": ["さうして", "さういふ", "さうだ"],
    "長音 よう→やう": ["やうな", "やうに", "やうす"],
    "長音 とう→たう": ["たうとう"],
    "長音 ちょう→ちやう": ["ちやうど"],
    "お→を": ["をもう", "をんな", "をとこ", "かをり"],
    "え→へ(語中)": ["こへ", "そへ"],
    "か→くわ": ["くわし", "くわじ"],
}


def main():
    works = {w["id"]: w for w in json.loads((ROOT / "data/works.json").read_text(encoding="utf-8"))["works"]}
    files = sorted(glob.glob(str(ROOT / "data/normalized/*.txt")))
    print(f"本文 {len(files):,} 作を走査…", flush=True)

    forms = sorted({f for v in PROBES.values() for f in v})
    total = collections.Counter()
    old = collections.Counter()

    t0 = time.time()
    for i, path in enumerate(files, 1):
        wid = os.path.basename(path)[:-4]
        is_old = works[wid]["kana"] in OLD_KANA
        text = io.open(path, encoding="utf-8").read()
        for f in forms:
            c = text.count(f)
            if c:
                total[f] += c
                if is_old:
                    old[f] += c
        if i % 1000 == 0:
            print(f"  {i:,}/{len(files):,}  {time.time() - t0:.0f}s", flush=True)

    result = {}
    print(f"\n{'規則':<20}{'件数':>10}{'旧仮名率':>10}  内訳")
    for rule, probes in PROBES.items():
        n = sum(total[f] for f in probes)
        o = sum(old[f] for f in probes)
        ratio = o / n if n else 0.0
        result[rule] = {
            "hits": n,
            "old_hits": o,
            "old_ratio": round(ratio, 4),
            "old_ratio_pct": int(ratio * 100),
            "probes": {f: {"hits": total[f], "old": old[f]} for f in probes},
        }
        detail = " ".join(f"{f}:{total[f]:,}" for f in probes if total[f])
        print(f"{rule:<20}{n:>10,}{ratio:>10.1%}  {detail}")

    OUT.write_text(
        json.dumps(
            {
                "works": len(files),
                "old_kana_works": sum(1 for w in works.values() if w["kana"] in OLD_KANA),
                "rules": result,
            },
            ensure_ascii=False,
            indent=1,
        ),
        encoding="utf-8",
    )
    print(f"\n→ {OUT.relative_to(ROOT)} ({time.time() - t0:.0f}s)")


if __name__ == "__main__":
    main()
