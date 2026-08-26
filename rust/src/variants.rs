//! 表記ゆれの展開(SPEC F-07)。
//!
//! 収録 5,000 作のうち **39% が旧仮名**(新字旧仮名 25% + 旧字旧仮名 15%)なので、
//! 「あわれ」で引いて「あはれ」が出ないのでは横断索引として成立しない。
//!
//! ## 辞書を持たない
//!
//! 歴史的仮名遣いへの変換は本来、語の切れ目と語種の知識を要する。しかしそれを
//! 辞書で与えると、辞書が「正しさ」の根拠になって検証が循環する(SPEC スコープ外)。
//!
//! そこでここは**機械的な置換だけ**を行い、規則の妥当性は台帳の
//! **文字遣い種別**(青空文庫が各作品に付けている独立した外部ラベル)で較正する。
//! ある異体形の出現が旧仮名の作品に偏っていれば、その規則は本物の仮名遣いを
//! 捉えている。偏っていなければ、それは助詞の境界などとの偶然の衝突である。
//!
//! ## 較正の実測(全 5,000 作・`scripts/calibrate_variants.py`)
//!
//! | 規則 | 旧仮名の作品に落ちる割合 | 件数 |
//! |---|--:|--:|
//! | い→ゐ | 99% | 58,583 |
//! | え→ゑ | 89% | 523 |
//! | じ→ぢ | 89% | 409 |
//! | ず→づ | 80% | 1,295 |
//! | う→ふ(語中) | 96% | 47,777 |
//! | お→ほ(語中) | 77% | 3,361 |
//! | わ→は(語中) | 84% | 1,606 |
//! | 長音 こう→かう | 99% | 2,013 |
//! | 長音 そう→さう | 99% | 8,394 |
//! | 長音 よう→やう | 99% | 46,152 |
//! | 長音 とう→たう | 97% | 430 |
//! | 長音 ちょう→ちやう | 99% | 514 |
//! | お→を | 37% | 937 |
//! | え→へ(語中) | 15% | 12,386 |
//! | か→くわ | 5% | 705 |
//!
//! 下 3 つは衝突が多い。「をもう」は本文の「…を、もう…」に、「こへ」は「どこへ」に、
//! 「くわし」は「食わし」に当たってしまう。**規則としては正しい**(をんな・かをりは
//! 本物の異体形)が、素朴な部分文字列照合では区別できない。語の切れ目を知らない
//! 以上これは解けないので、**既定では外し、形ごとに件数を見せて利用者が選べるように
//! する**。隠さずに出すのが本項の方針。

/// 置換規則
pub struct Rule {
    pub from: &'static str,
    pub to: &'static str,
    /// 語頭には適用しない(語中限定)
    pub medial: bool,
    /// 既定で有効か。較正で衝突が多いと分かったものは false
    pub default_on: bool,
    /// 較正で測った「旧仮名の作品に落ちる割合」(百分率)
    pub old_ratio: u8,
    pub note: &'static str,
}

const fn r(
    from: &'static str,
    to: &'static str,
    medial: bool,
    default_on: bool,
    old_ratio: u8,
    note: &'static str,
) -> Rule {
    Rule {
        from,
        to,
        medial,
        default_on,
        old_ratio,
        note,
    }
}

/// 長音の対応。お段+う → あ段+う(「かうして」「やうに」「さうだ」)
const LONG: &[(&str, &str)] = &[
    ("おう", "あう"),
    ("こう", "かう"),
    ("ごう", "がう"),
    ("そう", "さう"),
    ("ぞう", "ざう"),
    ("とう", "たう"),
    ("どう", "だう"),
    ("のう", "なう"),
    ("ほう", "はう"),
    ("ぼう", "ばう"),
    ("ぽう", "ぱう"),
    ("もう", "まう"),
    ("よう", "やう"),
    ("ろう", "らう"),
    ("きょう", "きやう"),
    ("ぎょう", "ぎやう"),
    ("しょう", "しやう"),
    ("じょう", "じやう"),
    ("ちょう", "ちやう"),
    ("にょう", "にやう"),
    ("ひょう", "ひやう"),
    ("びょう", "びやう"),
    ("ぴょう", "ぴやう"),
    ("みょう", "みやう"),
    ("りょう", "りやう"),
];

/// 単字の対応。`old_ratio` は `scripts/calibrate_variants.py` の実測値(百分率)。
///
/// 既定で有効にする閾値は **70%**。実測は 77.4%(お→ほ)と 37.0%(お→を)の間に
/// 大きな空きがあり、どこで切っても同じ分け方になる。
pub const RULES: &[Rule] = &[
    r("い", "ゐ", false, true, 99, "ゐる・ゐない・つひに"),
    r("え", "ゑ", false, true, 89, "こゑ・すゑ"),
    r("じ", "ぢ", false, true, 89, "すぢ・もみぢ・はぢめ"),
    r("ず", "づ", false, true, 80, "しづか・みづ・つづみ"),
    r("う", "ふ", true, true, 96, "いふ・おもふ(語中のみ)"),
    r("お", "ほ", true, true, 77, "なほ・こほり・とほい(語中のみ)"),
    r(
        "わ",
        "は",
        true,
        true,
        84,
        "かはいい・あはれ・かはり(語中のみ)",
    ),
    // --- 較正で衝突が多いと分かったもの。既定では外し、件数だけ見せる ---
    r(
        "お",
        "を",
        false,
        false,
        37,
        "をんな・かをり(『…を、もう…』とも当たる)",
    ),
    r(
        "え",
        "へ",
        true,
        false,
        15,
        "すゑ→すへ(『どこへ』とも当たる)",
    ),
    r(
        "か",
        "くわ",
        false,
        false,
        5,
        "くわし(『食わし』とも当たる)",
    ),
];

/// 既定で有効にする最低 old_ratio(百分率)
pub const DEFAULT_ON_THRESHOLD: u8 = 70;

/// 展開の上限。組み合わせ爆発を止める
pub const MAX_FORMS: usize = 48;

/// 語 `q` の異体形を返す。`include_risky` が false なら既定で外す規則を使わない。
///
/// 元の語そのものは含めない。順序は決定論的(規則順・出現位置順)。
pub fn expand(q: &str, include_risky: bool) -> Vec<String> {
    if q.is_empty() {
        return Vec::new();
    }
    let mut seen: Vec<String> = vec![q.to_string()];
    let mut frontier: Vec<String> = vec![q.to_string()];

    // 長音は 1 語に 1〜2 回しか出ないので先に当てる
    for round in 0..2 {
        let mut next = Vec::new();
        for s in &frontier {
            for (a, b) in LONG {
                for form in substitute(s, a, b, false) {
                    if !seen.contains(&form) && seen.len() < MAX_FORMS {
                        seen.push(form.clone());
                        next.push(form);
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
        let _ = round;
    }

    frontier = seen.clone();
    for _ in 0..2 {
        let mut next = Vec::new();
        for s in &frontier {
            for rule in RULES {
                if !rule.default_on && !include_risky {
                    continue;
                }
                for form in substitute(s, rule.from, rule.to, rule.medial) {
                    if !seen.contains(&form) && seen.len() < MAX_FORMS {
                        seen.push(form.clone());
                        next.push(form);
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }

    seen.remove(0); // 元の語を落とす
    seen
}

/// `s` の中の `a` を 1 箇所ずつ `b` に置き換えた形を、出現位置順に返す
fn substitute(s: &str, a: &str, b: &str, medial: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(i) = s[at..].find(a) {
        let pos = at + i;
        at = pos + a.len();
        if medial && pos == 0 {
            continue;
        }
        let mut t = String::with_capacity(s.len() + b.len());
        t.push_str(&s[..pos]);
        t.push_str(b);
        t.push_str(&s[pos + a.len()..]);
        out.push(t);
    }
    out
}
