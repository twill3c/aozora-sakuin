# -*- coding: utf-8 -*-
"""選定ゲート(SPEC §4)。`data/selection.tsv` の不変条件を固定する。

規則 v2 は順序に青空文庫のアクセスランキング(公開・外部指標)を使う。
v1(作品 ID 昇順)は決定論的だったが代表性を欠き、代表作 21 点のうち採録は 11 点、
芥川竜之介は羅生門も蜘蛛の糸も入らなかった。**決定論だけでは選定の質を保証しない**
ので、代表性そのものをゲートにする(G-12)。
"""
import collections
import csv
import hashlib
import io
import json
import pathlib
import subprocess
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
SELECTION = ROOT / "data/selection.tsv"
CATALOG = ROOT / "data/index_cache/list_person_all_extended_utf8.csv"
RANKING = ROOT / "data/ranking.json"
TARGET = 5000


def rows():
    with io.open(SELECTION, encoding="utf-8") as f:
        return list(csv.DictReader(f, delimiter="\t"))


def author(r):
    return r["姓"] + r["名"]


def counts():
    return collections.Counter(author(r) for r in rows())


def population():
    byid = {}
    with io.open(CATALOG, encoding="utf-8-sig") as f:
        for r in csv.DictReader(f):
            byid.setdefault(r["作品ID"], r)
    return {r["作品ID"].lstrip("0"): r for r in byid.values()
            if r.get("作品著作権フラグ") == "なし"
            and r.get("テキストファイルURL", "").endswith(".zip")}


# --- 形と量 ---

def test_g01_行数がちょうど5000():
    assert len(rows()) == TARGET


def test_g02_作品IDに重複がない():
    rs = rows()
    assert len({r["作品ID"] for r in rs}) == len(rs)


def test_g03_1著者17作を超えない():
    assert max(counts().values()) <= 17


def test_g04_17作の著者はちょうど75名():
    assert sum(1 for v in counts().values() if v == 17) == 75


def test_g05_著者921名を全員収録():
    assert len(counts()) == 921


def test_g10_作品ID昇順():
    ids = [int(r["作品ID"]) for r in rows()]
    assert ids == sorted(ids)


# --- 出所 ---

def test_g06_全行がzipのURLを持つ():
    assert all(r["テキストファイルURL"].endswith(".zip") for r in rows())


def test_g07_著作権フラグは全てなし():
    pop = population()
    assert all(r["作品ID"].lstrip("0") in pop for r in rows())


def test_g08_作品名が空の行がない():
    assert all(r["作品名"].strip() for r in rows())


def test_g11_URLはaozora_gr_jpのみ():
    assert all(r["テキストファイルURL"].startswith("https://www.aozora.gr.jp/") for r in rows())


# --- 決定論 ---

def test_g09_再実行で同一_決定論(tmp_path):
    """選定は seed も人手の恣意も持たない。再生成しても 1 バイト変わらない。"""
    before = hashlib.sha256(SELECTION.read_bytes()).hexdigest()
    out = tmp_path / "selection.tsv"
    subprocess.run([sys.executable, str(ROOT / "pipeline/select5000.py"), str(out)],
                   check=True, capture_output=True)
    assert hashlib.sha256(out.read_bytes()).hexdigest() == before


# --- 代表性(v2 で追加) ---

def test_g12_ランキング上位100作は母集団に在る限り必ず採録():
    """著者上限に阻まれない限り、よく読まれている作品は入っていなければならない。
    上位 200 位まで広げると 11 作が上限で落ちる(芥川の杜子春など)ので、
    上限に達しない範囲として 100 位を境にする。"""
    if not RANKING.exists():
        pytest.skip("data/ranking.json がない")
    rk = json.loads(RANKING.read_text(encoding="utf-8"))["works"]
    pop = population()
    sel = {r["作品ID"].lstrip("0") for r in rows()}
    top = [w for w in rk[:100] if w["id"] in pop]
    missing = [(w["id"], w["title"]) for w in top if w["id"] not in sel]
    assert not missing, f"上位 100 位のうち {len(missing)} 作が未採録: {missing[:5]}"


def test_g13_代表作が採録されている():
    """v1 が落としていた作品群。規則を変えたときに気づけるよう名指しで固定する。"""
    by = collections.defaultdict(set)
    for r in rows():
        by[author(r)].add(r["作品名"])
    canon = [
        ("夏目漱石", "吾輩は猫である"), ("夏目漱石", "こころ"), ("夏目漱石", "坊っちゃん"),
        ("芥川竜之介", "羅生門"), ("芥川竜之介", "蜘蛛の糸"), ("芥川竜之介", "地獄変"),
        ("太宰治", "走れメロス"), ("太宰治", "人間失格"), ("太宰治", "斜陽"),
        ("宮沢賢治", "銀河鉄道の夜"), ("宮沢賢治", "注文の多い料理店"), ("宮沢賢治", "風の又三郎"),
        ("森鴎外", "舞姫"), ("森鴎外", "高瀬舟"), ("樋口一葉", "たけくらべ"),
        ("坂口安吾", "堕落論"), ("梶井基次郎", "檸檬"), ("中島敦", "山月記"),
        ("小林多喜二", "蟹工船"), ("島崎藤村", "破戒"), ("国木田独歩", "武蔵野"),
    ]
    missing = [f"{a}『{t}』" for a, t in canon if t not in by[a]]
    assert not missing, f"代表作 {len(missing)} 点が未採録: {missing}"


def test_g14_順位スコア列が整合している():
    rk = json.loads(RANKING.read_text(encoding="utf-8"))["works"] if RANKING.exists() else []
    score = {w["id"]: w["score"] for w in rk}
    ranked = 0
    for r in rows():
        s = int(r["順位スコア"])
        want = score.get(r["作品ID"].lstrip("0"), 0)
        assert s == want, f"{r['作品ID']} のスコアが台帳と食い違う: {s} != {want}"
        ranked += s > 0
    assert ranked > 2000, f"ランキング掲載作が {ranked} 作しかない"
