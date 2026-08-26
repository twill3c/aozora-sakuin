//! 表記ゆれ展開のオラクル(SPEC F-07 / O-15)。
//!
//! 規則の**妥当性**は文字遣い種別で較正する(`tests/test_variants.py`)。
//! ここで見るのは展開そのものの性質 — 決定論・上限・語中限定・既知の対応。

use aozora_sakuin::variants::{expand, DEFAULT_ON_THRESHOLD, MAX_FORMS, RULES};

#[test]
fn o_15_展開は決定論的で元の語を含まない() {
    for q in ["あわれ", "ように", "しずか", "おもう", "こころ", "", "ん"] {
        let a = expand(q, false);
        let b = expand(q, false);
        assert_eq!(a, b, "「{q}」の展開が実行ごとに違う");
        assert!(
            !a.contains(&q.to_string()),
            "「{q}」の展開に元の語が入っている"
        );
        assert!(a.len() <= MAX_FORMS, "「{q}」で {} 形に膨らんだ", a.len());
        // 重複が無い
        let mut sorted = a.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), a.len(), "「{q}」の展開に重複がある");
    }
}

#[test]
fn o_15b_既知の対応が出る() {
    for (q, want) in [
        ("あわれ", "あはれ"),
        ("おもう", "おもふ"),
        ("ように", "やうに"),
        ("そうして", "さうして"),
        ("しずか", "しづか"),
        ("こえ", "こゑ"),
        ("いる", "ゐる"),
        ("ちょうど", "ちやうど"),
    ] {
        let forms = expand(q, false);
        assert!(
            forms.contains(&want.to_string()),
            "「{q}」から「{want}」が出ない: {forms:?}"
        );
    }
}

#[test]
fn o_15c_衝突しやすい規則は既定で出ない() {
    // 「をもう」は本文の「…を、もう…」に当たる。既定では出さない
    for (q, risky) in [("おもう", "をもう"), ("こえ", "こへ"), ("かし", "くわし")] {
        assert!(
            !expand(q, false).contains(&risky.to_string()),
            "既定で「{q}」から「{risky}」が出てしまう"
        );
        assert!(
            expand(q, true).contains(&risky.to_string()),
            "明示的に許しても「{q}」から「{risky}」が出ない"
        );
    }
}

#[test]
fn o_15d_語中限定の規則が語頭に当たらない() {
    // う→ふ は語中のみ。「うた」→「ふた」は誤り
    assert!(!expand("うた", false).contains(&"ふた".to_string()));
    assert!(!expand("おと", false).contains(&"ほと".to_string()));
    // 語中なら当たる
    assert!(expand("いう", false).contains(&"いふ".to_string()));
    assert!(expand("なお", false).contains(&"なほ".to_string()));
}

#[test]
fn o_15e_規則表と閾値が矛盾しない() {
    for rule in RULES {
        assert_eq!(
            rule.default_on,
            rule.old_ratio >= DEFAULT_ON_THRESHOLD,
            "{}→{} が閾値 {DEFAULT_ON_THRESHOLD}% と矛盾({}%)",
            rule.from,
            rule.to,
            rule.old_ratio
        );
    }
}

#[test]
fn o_15f_展開した形はすべて空でない文字列である() {
    for q in ["あわれ", "ように", "きょうじょう", "おもう", "しずかなよる"] {
        for f in expand(q, true) {
            assert!(!f.is_empty(), "「{q}」から空の形が出た");
            assert!(f.chars().count() > 0);
        }
    }
}
