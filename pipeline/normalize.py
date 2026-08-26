# -*- coding: utf-8 -*-
"""青空索引: 取得した本文を索引用に正規化する(SPEC F-02)。

除去するもの:
  1. ルビ《…》、ルビ開始記号｜、入力注記 ※［＃…］
  2. 冒頭の作品名・著者名(台帳の値と一致する行のみ — 貪欲マッチで本文を食わないため)
  3. 罫線に挟まれた凡例ブロック(【テキスト中に現れる記号について】等)
  4. 奥付(行頭が 底本：/底本の親本： の行から末尾まで)
  5. 残った罫線(凡例が非標準の版)

保持するもの:
  - 改行(行番号が用例の位置になる)
  - 本文中の記号・約物・字下げ

## 設計上の約束(HC-029)

各段の判定は位置・順序・割合ではなく**内容に対する述語**で書く。
「末尾 30% にある行」ではなく「行頭が 底本： である行」と書く。上流の段が行数を
変えても意味がずれないため。安全弁も同様に内容で置く。

## 処理順序(冪等性)

記法の除去は行末整形より**先**に行う。逆順にすると、注記を消した跡の全角空白が
行末に残り、2 度目の正規化で消えて冪等性が壊れる。
"""
import csv
import io
import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent
SELECTION = ROOT / "data/selection.tsv"
RAW = ROOT / "data/raw"
OUT = ROOT / "data/normalized"
WORKS = ROOT / "data/works.json"

DASH = re.compile(r"^-{20,}\s*$")
FOOTER_HEAD = re.compile(r"^(底本|底本の親本)[：:]")
FOOTER_ALT = re.compile(r"^(青空文庫作成ファイル|入力|校正)[：:]")
FOOTER_EVIDENCE = re.compile(r"^(入力|校正|青空文庫作成ファイル|ファイル作成)[：:]")
NOTE = re.compile(r"※?［＃[^］]*］")
RUBY = re.compile(r"《[^》]*》")
BLANKS = re.compile(r"\n{3,}")
FLAT = re.compile(r"[\s　]+")


def _flat(s):
    return FLAT.sub("", s)


def _strip_title(lines, title, author):
    """冒頭の作品名・著者名行を、台帳の値と一致する場合にのみ落とす。"""
    i = 0
    while i < len(lines) and not lines[i].strip():
        i += 1
    if i < len(lines) and title and _flat(lines[i]) == _flat(title):
        i += 1
        while i < len(lines) and not lines[i].strip():
            i += 1
        if i < len(lines) and _flat(lines[i]) in (_flat(author), _flat(author[::-1])):
            i += 1
        return lines[i:], "title+author"
    return lines, "none"


def _strip_legend(lines):
    """罫線に挟まれた凡例ブロックを落とす。

    罫線は本文側にも現れうる(実測 408 作中 5 作で 3 本以上)ため、ブロックの中身が
    凡例らしいこと(【 を含む、または「：ルビ」を含む)を条件にする。
    """
    idx = [i for i, l in enumerate(lines) if DASH.match(l)]
    if len(idx) >= 2:
        a, b = idx[0], idx[1]
        block = "\n".join(lines[a + 1:b])
        if "【" in block or "：ルビ" in block:
            return lines[b + 1:], "legend"
    return lines, "none"


def _strip_footer(lines):
    """奥付を落とす。

    切断点は「行頭が 底本：/底本の親本：」という内容の述語で決める。

    安全弁は**肯定的な証拠**で置く — 切り落とす側に入力者・校正者・作成ファイルの
    行が含まれること。奥付は自由記述の注記(初出、旧字旧仮名についての断り、
    編集部の傍注についての説明…)を含む開いた集合なので、「奥付らしい行だけで
    構成されている」という否定的な網羅検査は必ず破れる(実測 816 作で 360 作が誤拒否)。
    証拠を探す向きに変えると 814/816 が通り、残り 2 作だけが要判断として残る。
    """
    cut = None
    for i, line in enumerate(lines):
        if FOOTER_HEAD.match(line):
            cut = i
            break
    if cut is None:                       # 底本：を持たない版がある(実測 816 作中 1 作)
        for i, line in enumerate(lines):
            if FOOTER_ALT.match(line):
                cut = i
                break
    if cut is None:
        return lines, "none"
    if not any(FOOTER_EVIDENCE.match(l) for l in lines[cut:]):
        return lines, "refused"           # 奥付である証拠がない — 誤爆を疑って手を触れない
    return lines[:cut], "footer"


def _dropped(before, after):
    """行リストから落ちた分の文字数(改行 1 個ずつを含む)。"""
    return sum(len(l) + 1 for l in before[:len(before) - len(after)]) if after is not before else 0


def normalize(text, title="", author=""):
    """本文を索引用に正規化し、(本文, 各段の適用結果, 除去量の内訳) を返す。冪等。

    内訳は検算のためにある。除去した文字数の合計と残った本文の長さを足すと、
    入力の長さに**厳密に一致**しなければならない(ゲート N-09)。残存率の閾値では
    「総ルビの底本でルビが原文の 47% を占める」ような正当な事例と、除去器の
    暴走とを区別できない。収支が合うかどうかなら区別できる。
    """
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    n_in = len(text)

    t = NOTE.sub("", text)
    acc = {"note": n_in - len(t)}
    u = RUBY.sub("", t)
    acc["ruby"] = len(t) - len(u)
    v = u.replace("｜", "")
    acc["pipe"] = len(u) - len(v)

    raw_lines = v.split("\n")
    lines = [l.rstrip() for l in raw_lines]
    acc["trailing_ws"] = sum(len(a) - len(b) for a, b in zip(raw_lines, lines))

    before = lines
    lines, f_title = _strip_title(lines, title, author)
    acc["title"] = sum(len(l) + 1 for l in before[:len(before) - len(lines)])

    before = lines
    lines, f_legend = _strip_legend(lines)
    acc["legend"] = sum(len(l) + 1 for l in before[:len(before) - len(lines)])

    before = lines
    lines, f_footer = _strip_footer(lines)
    acc["footer"] = sum(len(l) + 1 for l in before[len(lines):])

    before = lines
    lines = [l for l in lines if not DASH.match(l)]
    acc["dash"] = sum(len(l) + 1 for l in before if DASH.match(l))

    joined = "\n".join(lines)
    collapsed = BLANKS.sub("\n\n", joined)
    acc["blank"] = len(joined) - len(collapsed)
    body = collapsed.strip("\n")
    acc["edge"] = len(collapsed) - len(body)

    acc["in"] = n_in
    acc["out"] = len(body)
    acc["balance"] = n_in - len(body) - sum(
        acc[k] for k in ("note", "ruby", "pipe", "trailing_ws",
                         "title", "legend", "footer", "dash", "blank", "edge"))
    return body + "\n", {"title": f_title, "legend": f_legend, "footer": f_footer}, acc


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    with io.open(SELECTION, encoding="utf-8") as f:
        rows = {r["作品ID"]: r for r in csv.DictReader(f, delimiter="\t")}

    # 選定から外れた作品の正規化済みファイルを消す。
    # 残しておくとシャードに混ざり、選定に無い作品が索引に入る
    stale = [p for p in OUT.glob("*.txt") if p.stem not in rows]
    for p in stale:
        p.unlink()
    if stale:
        print(f"選定外になった正規化済み {len(stale)} 作を削除")

    works, excluded, skipped = [], [], 0
    for path in sorted(RAW.glob("*.txt")):
        wid = path.stem
        r = rows.get(wid)
        if r is None:
            skipped += 1
            continue
        raw = path.read_text(encoding="utf-8")
        body, forms, acc = normalize(raw, r["作品名"], r["姓"] + r["名"])
        if forms["footer"] != "footer":
            # 奥付を切り落とせなかった作品は索引に入れない。書誌が本文として
            # 索引に入ると、用例に奥付由来の偽の一致が出る
            excluded.append({"id": wid, "title": r["作品名"], "reason": forms["footer"]})
            (OUT / f"{wid}.txt").unlink(missing_ok=True)
            continue
        (OUT / f"{wid}.txt").write_text(body, encoding="utf-8", newline="\n")
        works.append({
            "id": wid, "title": r["作品名"], "author": r["姓"] + r["名"],
            "kana": r["文字遣い種別"], "ndc": (r["分類番号"] or "").split("/")[0].strip(),
            "raw_chars": len(raw), "chars": len(body),
            "lines": body.count("\n"), "utf8": len(body.encode("utf-8")),
            "forms": forms, "acc": acc,
        })

    works.sort(key=lambda w: int(w["id"]))
    WORKS.write_text(json.dumps({"n": len(works), "excluded": excluded, "works": works},
                                ensure_ascii=False), encoding="utf-8")
    ch = sum(w["chars"] for w in works)
    ub = sum(w["utf8"] for w in works)
    raw_total = sum(w["raw_chars"] for w in works)
    print(f"正規化 {len(works):,} 作 / 対象外 {skipped}")
    print(f"  正味 {ch:,} 字 / {ub / 1e6:.1f} MB(UTF-8) / 残存率 {ch / raw_total:.1%}")
    none = sum(1 for w in works if w["forms"]["legend"] == "none")
    print(f"  凡例ブロックを検出できず {none} 作(非標準の版)")
    print(f"  奥付を落とせず索引から除外 {len(excluded)} 作 "
          f"({len(excluded) / (len(works) + len(excluded)):.2%}): {excluded[:3]}")


if __name__ == "__main__":
    main()
