# -*- coding: utf-8 -*-
"""正規化ゲート(SPEC F-02)。

検査は多段処理の**最終出力**に対して掛ける(段ごとではなく — HC-029)。
中心は N-09 の収支検算で、残存率の閾値は補助にすぎない。閾値では
「総ルビの底本でルビが原文の 47% を占める」正当な事例と除去器の暴走を
区別できないが、収支なら区別できる。
"""
import io
import json
import pathlib
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
NORM = ROOT / "data/normalized"
WORKS = ROOT / "data/works.json"

sys.path.insert(0, str(ROOT))
from pipeline.normalize import normalize  # noqa: E402


def texts():
    fs = sorted(NORM.glob("*.txt"))
    if not fs:
        pytest.skip("data/normalized が空(取得・正規化がまだ)")
    return {f.stem: f.read_text(encoding="utf-8") for f in fs}


def doc():
    if not WORKS.exists():
        pytest.skip("data/works.json がない")
    return json.loads(WORKS.read_text(encoding="utf-8"))


def works():
    return doc()["works"]


# --- 残骸検査(取りこぼしを直接見に行く。可逆性検査と違い恒等にならない — HC-027) ---

@pytest.mark.parametrize("mark,label", [
    ("《", "ルビ開始"), ("》", "ルビ終了"), ("｜", "ルビ位置指定"),
    ("［＃", "入力注記"), ("【テキスト中に現れる記号について】", "凡例見出し"),
])
def test_n01_記法が残っていない(mark, label):
    bad = [k for k, t in texts().items() if mark in t]
    assert not bad, f"{label} が {len(bad)} 作に残留: {bad[:5]}"


def test_n02_奥付が残っていない():
    bad = [k for k, t in texts().items()
           if any(l.startswith(("底本：", "底本の親本：", "青空文庫作成ファイル："))
                  for l in t.split("\n"))]
    assert not bad, f"奥付が {len(bad)} 作に残留: {bad[:5]}"


def test_n03_区切り線が残っていない():
    bad = [k for k, t in texts().items()
           if any(l.strip().startswith("-" * 20) for l in t.split("\n"))]
    assert not bad, f"罫線が {len(bad)} 作に残留: {bad[:5]}"


# --- 収支検算(中心のオラクル) ---

def test_n09_除去量の収支が厳密に合う():
    """入力の長さ = 残った本文 + 除去した各分類の合計。1 文字のずれも許さない。

    これが通れば「消えた文字は必ずどれかの分類に計上されている」ことになり、
    除去器が本文を黙って食う経路がなくなる。
    """
    bad = [(w["id"], w["acc"]["balance"]) for w in works() if w["acc"]["balance"] != 0]
    assert not bad, f"収支不一致 {len(bad)} 作: {bad[:5]}"


def test_n10_除去の内訳が想定の分類に収まる():
    """本文の大半を占めるのはルビと入力注記であるはず。分類の比が大きく崩れたら
    除去器がどこかで想定外のものを食っている。"""
    ws = works()
    tot = {}
    for w in ws:
        for k, v in w["acc"].items():
            if k not in ("in", "out", "balance"):
                tot[k] = tot.get(k, 0) + v
    s = sum(tot.values()) or 1
    assert tot["ruby"] / s > 0.4, f"ルビの比率 {tot['ruby'] / s:.1%} が想定外に低い"
    assert tot["title"] / s < 0.05, f"題名除去の比率 {tot['title'] / s:.1%} が想定外に高い"
    assert tot["blank"] / s < 0.10, f"空行圧縮の比率 {tot['blank'] / s:.1%} が想定外に高い"


# --- 過剰除去・除外の検査 ---

def test_n04_本文が丸ごと消えた作品がない():
    """ゲートの目的は「除去器が本文を食い尽くす暴走」の検出であって、短い作品の排除ではない。

    絶対字数の閾値は前のコーパスに依存する(v1 で決めた 50 字が、短詩を含む v2 で
    偽陽性を出した)。内容の述語で書く — 正味が 0 字でないこと、そして短い作品は
    生テキストも短いこと(短さの原因が底本の側にあること)。実測の短詩 9 作は
    正味 37〜49 字・生 264〜644 字。
    """
    ws = works()
    empty = [w["id"] for w in ws if w["chars"] == 0]
    assert not empty, f"本文が空の作品 {len(empty)} 作: {empty[:5]}"
    suspicious = [(w["id"], w["raw_chars"], w["chars"]) for w in ws
                  if w["chars"] < 50 and w["raw_chars"] > 2000]
    assert not suspicious,         f"生が長いのに正味が 50 字未満 = 除去器の暴走を疑う {len(suspicious)} 作: {suspicious[:5]}"


def test_n07_索引から除外された作品が1パーセント未満():
    """奥付を落とせなかった作品は索引に入れない(奥付由来の偽の一致を避けるため)。
    ただし増えたら正規化側の問題なので割合を固定する。"""
    d = doc()
    ex, n = d.get("excluded", []), d["n"]
    assert len(ex) / max(1, n + len(ex)) < 0.01, f"除外 {len(ex)} 作 / 収録 {n} 作: {ex[:5]}"


# --- 冪等性 ---

def test_n08_冪等():
    for wid, t in list(texts().items())[:200]:
        again, _, _ = normalize(t)
        assert again == t, f"{wid} で normalize が冪等でない"


def test_n11_選定にない作品が混ざっていない():
    """選定を差し替えると、前の選定の正規化済みファイルが残る。
    残ったまま索引を組むと、選定に無い作品が引けてしまう。"""
    import csv as _csv
    sel = {r["作品ID"] for r in _csv.DictReader(
        io.open(ROOT / "data/selection.tsv", encoding="utf-8"), delimiter="\t")}
    stray = [k for k in texts() if k not in sel]
    assert not stray, f"選定にない作品が {len(stray)} 作: {stray[:5]}"
