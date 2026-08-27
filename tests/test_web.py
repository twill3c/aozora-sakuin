# -*- coding: utf-8 -*-
"""画面のゲート(SPEC F-05 / F-06 / N-01 / N-02)。

ブラウザを起動せずに確かめられることを固定する。とくに **N-02「JS は索引の
構造を知らない」** は放っておくと崩れるので、構造として検査する。
"""
import csv
import io
import json
import pathlib
import re

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEB = ROOT / "web"
HTML = WEB / "index.html"
JS = WEB / "app.js"
CSS = WEB / "app.css"
MANIFEST = WEB / "index/manifest.json"
WORKS = WEB / "index/works.json"
SHARD_RS = ROOT / "rust/src/shard.rs"


def html():
    return HTML.read_text(encoding="utf-8")


def js():
    return JS.read_text(encoding="utf-8")


def test_w01_jsが参照するidがhtmlにある():
    ids = set(re.findall(r'\$\("([^"]+)"\)', js()))
    have = set(re.findall(r'id="([^"]+)"', html()))
    missing = sorted(ids - have)
    assert not missing, f"app.js が参照する id が index.html に無い: {missing}"


def test_w02_htmlが参照する資材が存在する():
    for path in re.findall(r'(?:href|src)="((?!https?:)[^"]+)"', html()):
        assert (WEB / path).exists(), f"{path} が無い"
    assert (WEB / "wasm/sakuin.wasm").exists(), "wasm が置かれていない"


def strip_comments(src):
    """コメントを落とす。索引の内部語は『JS 側に無い』ことを説明する注記には現れるので、
    検査するのはコードだけにする。"""
    src = re.sub(r"/\*.*?\*/", "", src, flags=re.S)
    return "\n".join(re.sub(r"//.*$", "", line) for line in src.split("\n"))


def test_w03_jsは索引の構造を知らない():
    """N-01 / N-02。BWT もウェーブレット木も rank も JS 側に漏れていないこと。
    漏れ始めると『Rust 工房・JS 店先』の切り分けが崩れる。"""
    src = strip_comments(js()).lower()
    banned = ["bwt", "wavelet", "ウェーブレット", "suffix", "接尾辞", "rank1", "lf写像", "sa_sample"]
    found = [b for b in banned if b in src]
    assert not found, f"索引の内部語が JS に現れている: {found}"
    # wasm へ触るのは az_ で始まる輸出だけ
    calls = set(re.findall(r"az\.([A-Za-z_][A-Za-z0-9_]*)", strip_comments(js())))
    bad = sorted(c for c in calls if not c.startswith("az_") and c != "memory")
    assert not bad, f"az_ 以外の輸出を触っている: {bad}"


def test_w04_配信形式の版が索引と照合器で一致する():
    if not MANIFEST.exists():
        pytest.skip("web/index/manifest.json が無い(先に build_shards)")
    want = int(re.search(r"VERSION: u32 = (\d+)", SHARD_RS.read_text(encoding="utf-8")).group(1))
    got = json.loads(MANIFEST.read_text(encoding="utf-8"))["format_version"]
    assert got == want, f"manifest の版 {got} と Rust の VERSION {want} が違う"


def test_w05_台帳の作品がすべて選定に含まれる():
    if not WORKS.exists():
        pytest.skip("web/index/works.json が無い")
    meta = json.loads(WORKS.read_text(encoding="utf-8"))["works"]
    sel = {int(r["作品ID"]) for r in csv.DictReader(
        io.open(ROOT / "data/selection.tsv", encoding="utf-8"), delimiter="\t")}
    stray = [k for k in meta if int(k) not in sel]
    assert not stray, f"選定にない作品が台帳にある: {stray[:5]}"


def test_w06_シャードの総和が台帳の作品数と合う():
    if not MANIFEST.exists():
        pytest.skip("web/index/manifest.json が無い")
    m = json.loads(MANIFEST.read_text(encoding="utf-8"))
    total = sum(s["docs"] for s in m["shards"])
    assert total == m["works"], f"シャードの作品数の合計 {total} != 収録 {m['works']}"
    meta = json.loads(WORKS.read_text(encoding="utf-8"))["works"]
    assert len(meta) == m["works"], f"台帳 {len(meta)} 作 != 収録 {m['works']} 作"


def test_w07_配信の量が基準に収まる():
    """SPEC O-05。1 枚が大きすぎると初回表示が遅くなる。"""
    if not MANIFEST.exists():
        pytest.skip("web/index/manifest.json が無い")
    m = json.loads(MANIFEST.read_text(encoding="utf-8"))
    biggest = max(s["bytes"] for s in m["shards"])
    assert biggest < 8_000_000, f"最大のシャードが {biggest/1e6:.1f} MB"
    first_load = (WEB / "wasm/sakuin.wasm").stat().st_size + WORKS.stat().st_size + MANIFEST.stat().st_size
    assert first_load < 1_500_000, f"初回に落とす資材が {first_load/1e6:.2f} MB"


def test_w08_暗い配色と明るい配色の両方が定義されている():
    src = CSS.read_text(encoding="utf-8")
    assert "prefers-color-scheme: dark" in src, "暗い配色が無い"
    # 色は必ずトークン経由。メディアクエリの中だけで定義された色があると片方が壊れる
    root_vars = set(re.findall(r"--([a-z-]+):", src.split("@media")[0]))
    dark_block = src.split("prefers-color-scheme: dark")[1].split("}\n}")[0]
    dark_vars = set(re.findall(r"--([a-z-]+):", dark_block))
    only_dark = sorted(dark_vars - root_vars)
    assert not only_dark, f"暗い配色でしか定義されていない色がある: {only_dark}"


def test_w09_配信物に台帳が指さないファイルがない():
    """組み直しで枚数が変わると、前の生成物が残る。manifest が指さないファイルが
    配信物に紛れ込むと、無駄に上がるうえ何が本物か分からなくなる(実際に 175 枚残った)。"""
    if not MANIFEST.exists():
        pytest.skip("web/index/manifest.json が無い")
    m = json.loads(MANIFEST.read_text(encoding="utf-8"))
    listed = {s["file"] for s in m["shards"]}
    on_disk = {p.name for p in (WEB / "index").glob("*.azsk")}
    stray = sorted(on_disk - listed)
    missing = sorted(listed - on_disk)
    assert not stray, f"台帳にないシャードが {len(stray)} 枚: {stray[:5]}"
    assert not missing, f"台帳にあるのに無いシャードが {len(missing)} 枚: {missing[:5]}"


def test_w10_配信物の総量が想定に収まる():
    """web/ 全体の大きさ。残骸が混ざると跳ね上がる。"""
    if not MANIFEST.exists():
        pytest.skip("web/index/manifest.json が無い")
    total = sum(p.stat().st_size for p in WEB.rglob("*") if p.is_file())
    m = json.loads(MANIFEST.read_text(encoding="utf-8"))
    # 索引 + 台帳 + wasm + 画面。索引の 1.05 倍を超えたら何かが余分に混ざっている
    assert total < m["total_bytes"] * 1.05, \
        f"web/ が {total/1e6:.0f} MB / 索引は {m['total_bytes']/1e6:.0f} MB"
