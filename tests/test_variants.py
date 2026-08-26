# -*- coding: utf-8 -*-
"""表記ゆれ規則のゲート(SPEC F-07 / O-17)。

規則表(`rust/src/variants.rs`)に書いてある old_ratio は、`data/variant_calibration.json`
の実測値でなければならない。**HC-030 に従い、閾値は「どのデータで較正されたか」と
必ず対で持つ。** 表と実測が離れたらここで落ちる。
"""
import json
import pathlib
import re

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
RULES_RS = ROOT / "rust/src/variants.rs"
CALIB = ROOT / "data/variant_calibration.json"

# 規則表の並び順と、較正ファイルの規則名の対応
RULE_KEYS = [
    "い→ゐ", "え→ゑ", "じ→ぢ", "ず→づ",
    "う→ふ(語中)", "お→ほ(語中)", "わ→は(語中)",
    "お→を", "え→へ(語中)", "か→くわ",
]

RULE_RE = re.compile(
    r'r\(\s*"([^"]+)",\s*"([^"]+)",\s*(true|false),\s*(true|false),\s*(\d+),',
    re.S,
)


def rust_rules():
    src = RULES_RS.read_text(encoding="utf-8")
    body = src[src.index("pub const RULES"):src.index("pub const DEFAULT_ON_THRESHOLD")]
    return [
        {"from": m[0], "to": m[1], "medial": m[2] == "true",
         "default_on": m[3] == "true", "old_ratio": int(m[4])}
        for m in RULE_RE.findall(body)
    ]


def threshold():
    src = RULES_RS.read_text(encoding="utf-8")
    return int(re.search(r"DEFAULT_ON_THRESHOLD: u8 = (\d+)", src).group(1))


def calib():
    if not CALIB.exists():
        pytest.skip("data/variant_calibration.json がない(scripts/calibrate_variants.py)")
    return json.loads(CALIB.read_text(encoding="utf-8"))


def test_v01_規則表の件数が較正と対応している():
    rules = rust_rules()
    assert len(rules) == len(RULE_KEYS), f"規則が {len(rules)} 個 / 想定 {len(RULE_KEYS)} 個"


def test_v02_old_ratioが実測値と一致する():
    """表に書いた数字が、実際に測った値であること。
    書いた時点の実測と離れたら落ちる — 閾値を根拠なく動かせないようにする(HC-030)。"""
    c = calib()["rules"]
    bad = []
    for rule, key in zip(rust_rules(), RULE_KEYS):
        want = c[key]["old_ratio_pct"]
        if abs(rule["old_ratio"] - want) > 1:
            bad.append(f'{key}: 表 {rule["old_ratio"]}% / 実測 {want}%')
    assert not bad, "規則表と較正が食い違う: " + " / ".join(bad)


def test_v03_既定の可否が閾値と矛盾しない():
    th = threshold()
    bad = []
    for rule, key in zip(rust_rules(), RULE_KEYS):
        should = rule["old_ratio"] >= th
        if rule["default_on"] != should:
            bad.append(f'{key}: {rule["old_ratio"]}% なのに default_on={rule["default_on"]}')
    assert not bad, f"閾値 {th}% と規則の可否が矛盾: " + " / ".join(bad)


def test_v04_閾値が実測の空きの中にある():
    """閾値は恣意的に置かない。採用組の最小値と不採用組の最大値の間にあること。
    間が詰まってきたら、閾値ではなく規則の立て方を見直す合図になる。"""
    th = threshold()
    rules = rust_rules()
    on = [r["old_ratio"] for r in rules if r["default_on"]]
    off = [r["old_ratio"] for r in rules if not r["default_on"]]
    assert max(off) < th <= min(on), \
        f"閾値 {th}% が空き({max(off)}%〜{min(on)}%)の中にない"
    assert min(on) - max(off) >= 20, \
        f"採用 {min(on)}% と不採用 {max(off)}% の差が {min(on) - max(off)} 点しかない — 分け方を見直すこと"


def test_v05_較正が現行のコーパスで測られている():
    c = calib()
    works = json.loads((ROOT / "data/works.json").read_text(encoding="utf-8"))["n"]
    assert c["works"] == works, \
        f"較正は {c['works']:,} 作で測ったが、いまのコーパスは {works:,} 作 — 測り直すこと"


def test_v06_衝突しやすい規則が既定で外れている():
    """『既定で入れない』ことそのものを固定する。うっかり on にすると
    「こへ」12,000 件のような衝突が結果に混ざる。"""
    off = {(r["from"], r["to"]) for r in rust_rules() if not r["default_on"]}
    for pair in [("お", "を"), ("え", "へ"), ("か", "くわ")]:
        assert pair in off, f"{pair[0]}→{pair[1]} が既定で有効になっている"
