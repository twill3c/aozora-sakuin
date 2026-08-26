//! 探索のオラクル(SPEC O-01)。
//!
//! FM-index の探索結果が、総当たり探索の**全ヒット位置と完全に一致**すること。
//! 件数だけでなく位置まで突き合わせる — 件数だけなら、区間の幅を取り違えていても
//! 偶然一致しうる。

use aozora_sakuin::bitvec::BitVec;
use aozora_sakuin::fm::{find_all_naive, FmIndex};
use aozora_sakuin::wavelet::WaveletTree;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

fn random_bytes(rng: &mut Rng, len: usize, alphabet: u8) -> Vec<u8> {
    (0..len)
        .map(|_| 1 + rng.below(alphabet as u64) as u8)
        .collect()
}

// ---------------------------------------------------------------- 下位構造

#[test]
fn o_bv_01_rank1が総当たりと一致する() {
    let mut rng = Rng::new(11);
    for len in [0usize, 1, 63, 64, 65, 511, 512, 513, 5000] {
        let bits: Vec<bool> = (0..len).map(|_| rng.below(2) == 1).collect();
        let bv = BitVec::from_bits(&bits);
        for i in 0..=len {
            assert_eq!(
                bv.rank1(i),
                BitVec::rank1_naive(&bits, i),
                "len={len} i={i}"
            );
        }
    }
}

#[test]
fn o_wt_01_rankとaccessが総当たりと一致する() {
    let mut rng = Rng::new(13);
    for &alphabet in &[1u8, 2, 5, 255] {
        for len in [1usize, 2, 33, 200, 1000] {
            let s = random_bytes(&mut rng, len, alphabet);
            let wt = WaveletTree::new(&s);
            for (i, &want) in s.iter().enumerate() {
                assert_eq!(
                    wt.access(i),
                    want,
                    "access alphabet={alphabet} len={len} i={i}"
                );
            }
            // rank は全文字 × 全位置だと重いので、出現する文字に絞って全位置を見る
            let mut present: Vec<u8> = s.clone();
            present.sort_unstable();
            present.dedup();
            for &c in &present {
                for i in 0..=len {
                    let want = s[..i].iter().filter(|&&x| x == c).count();
                    assert_eq!(wt.rank(c, i), want, "rank c={c} i={i} len={len}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------- O-01

#[test]
fn o_01_無作為な本文と語で全ヒット位置が一致する() {
    let mut rng = Rng::new(20_260_826);
    let mut checked = 0usize;
    for &alphabet in &[2u8, 3, 8, 60] {
        for len in [1usize, 2, 17, 64, 300, 1500] {
            let text = random_bytes(&mut rng, len, alphabet);
            let fm = FmIndex::build(&text);
            for plen in 1..=6usize.min(len) {
                for _ in 0..8 {
                    // 本文から切り出した語(必ず当たる)と、無作為な語(当たらないことが多い)
                    let pat = if rng.below(2) == 0 && len >= plen {
                        let at = rng.below((len - plen + 1) as u64) as usize;
                        text[at..at + plen].to_vec()
                    } else {
                        random_bytes(&mut rng, plen, alphabet)
                    };
                    let want = find_all_naive(&text, &pat);
                    assert_eq!(
                        fm.count(&pat),
                        want.len(),
                        "件数 text_len={len} pat={pat:?}"
                    );
                    assert_eq!(fm.locate(&pat), want, "位置 text_len={len} pat={pat:?}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked >= 500, "検査した組が {checked} 件しかない");
}

#[test]
fn o_01b_日本語の実文で全ヒット位置が一致する() {
    let text = "春はあけぼの。やうやう白くなりゆく山ぎは、すこしあかりて、\
                 むらさきだちたる雲のほそくたなびきたる。\
                 夏は夜。月のころはさらなり、闇もなほ、蛍の多く飛びちがひたる。\
                 秋は夕暮。夕日のさして山の端いと近うなりたるに、\
                 烏の寝どころへ行くとて、三つ四つ、二つ三つなど飛びいそぐさへあはれなり。\
                 冬はつとめて。雪の降りたるはいふべきにもあらず。";
    let bytes = text.as_bytes();
    let fm = FmIndex::build(bytes);
    for pat in [
        "は",
        "あけぼの",
        "なり",
        "山",
        "夕暮",
        "あはれ",
        "。",
        "たる",
        "見当",
        "ゐ",
    ] {
        let p = pat.as_bytes();
        let want = find_all_naive(bytes, p);
        assert_eq!(fm.count(p), want.len(), "件数 pat={pat}");
        assert_eq!(fm.locate(p), want, "位置 pat={pat}");
    }
}

#[test]
fn o_01c_一致位置は必ず文字境界にある() {
    // UTF-8 は自己同期的なので、バイト単位で探しても文字の途中には当たらない。
    // これが「索引をバイト列に張ってよい」根拠なので、根拠のほうを検査する。
    let text = "吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。";
    let bytes = text.as_bytes();
    let fm = FmIndex::build(bytes);
    for pat in ["は", "猫", "である", "。", "名前"] {
        for &pos in &fm.locate(pat.as_bytes()) {
            assert!(
                text.is_char_boundary(pos),
                "pat={pat} の一致位置 {pos} が文字境界でない"
            );
        }
    }
}

#[test]
fn o_01d_退化した本文でも一致する() {
    for text in [
        vec![7u8; 200],
        b"abababababababababab".to_vec(),
        b"aaaaabaaaaabaaaaab".to_vec(),
        b"mississippi".to_vec(),
    ] {
        let fm = FmIndex::build(&text);
        for plen in 1..=5usize.min(text.len()) {
            for at in 0..=text.len() - plen {
                let pat = &text[at..at + plen];
                assert_eq!(fm.locate(pat), find_all_naive(&text, pat), "pat={pat:?}");
            }
        }
    }
}

// ---------------------------------------------------------------- O-08

#[test]
fn o_08_位置復元の歩数に実測の上界がある() {
    // 行標本には理論上の上界が無い(位置標本なら SA_SAMPLE 歩で必ず当たる)。
    // 13.3% の容量と引き換えに手放した保証なので、実測で押さえて回帰を検出する。
    //
    // 20 MB の実本文での実測は最大 846 歩(SA_SAMPLE = 64 の 13 倍)。
    // ここでは軽い入力で回し、桁が変わったら落ちるようにしておく。
    use aozora_sakuin::fm::SA_SAMPLE;
    let mut rng = Rng::new(101);
    for &alphabet in &[2u8, 5, 60] {
        for len in [1000usize, 20_000] {
            let text = random_bytes(&mut rng, len, alphabet);
            let fm = FmIndex::build(&text);
            let steps = fm.max_locate_steps();
            assert!(
                steps <= SA_SAMPLE * 100,
                "alphabet={alphabet} len={len} で最大 {steps} 歩 — 上界 {} を超えた",
                SA_SAMPLE * 100
            );
        }
    }
}

#[test]
fn o_08b_偏った本文でも歩数が発散しない() {
    // 同じ文字が延々と続く本文は LF が長い鎖になりやすい
    use aozora_sakuin::fm::SA_SAMPLE;
    for text in [vec![7u8; 20_000], b"ab".repeat(10_000)] {
        let fm = FmIndex::build(&text);
        let steps = fm.max_locate_steps();
        assert!(
            steps <= SA_SAMPLE * 100,
            "最大 {steps} 歩 — 上界 {} を超えた",
            SA_SAMPLE * 100
        );
    }
}

// ---------------------------------------------------------------- O-09

#[test]
fn o_09_配信形式に書き出して読み直しても同じ結果になる() {
    use aozora_sakuin::shard::{Doc, Shard};
    let mut rng = Rng::new(4649);
    for &alphabet in &[2u8, 5, 60] {
        for len in [1usize, 100, 5000] {
            let text = random_bytes(&mut rng, len, alphabet);
            let docs = vec![
                Doc { id: 1, offset: 0 },
                Doc {
                    id: 2,
                    offset: (len / 2) as u32,
                },
            ];
            let shard = Shard::build(&text, docs.clone());
            let bytes = shard.to_bytes();
            let back = Shard::from_bytes(&bytes).expect("読み直せない");
            assert_eq!(back.docs, docs);
            for plen in 1..=4usize.min(len) {
                for _ in 0..10 {
                    let at = rng.below((len - plen + 1) as u64) as usize;
                    let pat = &text[at..at + plen];
                    let want = find_all_naive(&text, pat);
                    assert_eq!(shard.fm.locate(pat), want, "メモリ上 pat={pat:?}");
                    assert_eq!(back.fm.locate(pat), want, "読み直し後 pat={pat:?}");
                }
            }
        }
    }
}

#[test]
fn o_09b_日本語の実文で配信形式が往復する() {
    use aozora_sakuin::shard::{Doc, Shard};
    let a = "春はあけぼの。やうやう白くなりゆく山ぎは、すこしあかりて。";
    let b = "吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。";
    let text = format!("{a}{b}");
    let bytes_text = text.as_bytes();
    let docs = vec![
        Doc { id: 1, offset: 0 },
        Doc {
            id: 2,
            offset: a.len() as u32,
        },
    ];
    let shard = Shard::build(bytes_text, docs);
    let back = Shard::from_bytes(&shard.to_bytes()).unwrap();
    for pat in ["は", "あけぼの", "猫", "。", "ゐ"] {
        let p = pat.as_bytes();
        let want = find_all_naive(bytes_text, p);
        assert_eq!(back.fm.locate(p), want, "pat={pat}");
        assert_eq!(back.fm.count(p), want.len(), "pat={pat}");
    }
    // 位置から作品を引けること
    for &pos in &back.fm.locate("猫".as_bytes()) {
        assert_eq!(
            back.docs[back.doc_of(pos)].id,
            2,
            "猫 は 2 番目の作品にある"
        );
    }
    for &pos in &back.fm.locate("あけぼの".as_bytes()) {
        assert_eq!(back.docs[back.doc_of(pos)].id, 1);
    }
}

#[test]
fn o_09c_シャード割りは作品をまたがない() {
    use aozora_sakuin::shard::plan_shards;
    let sizes = vec![10, 20, 5, 100, 3, 4, 60, 7];
    for target in [1usize, 20, 30, 1000] {
        let plan = plan_shards(&sizes, target);
        // 全作品がちょうど 1 回ずつ現れる
        let mut covered = Vec::new();
        for &(a, b) in &plan {
            assert!(a < b, "空のシャード");
            covered.extend(a..b);
        }
        assert_eq!(
            covered,
            (0..sizes.len()).collect::<Vec<_>>(),
            "target={target}"
        );
        // 2 作以上入っているシャードは target を超えない
        for &(a, b) in &plan {
            let sum: usize = sizes[a..b].iter().sum();
            assert!(
                sum <= target || b - a == 1,
                "target={target} で {sum} バイト"
            );
        }
    }
}

// ---------------------------------------------------------------- O-11

#[test]
fn o_11_索引だけから本文を復元できる() {
    // 原文を配信しないので、前後文脈は索引から取り出すほかない。
    // 取り出した結果が原文と**バイト単位で一致**することを、全区間で確かめる。
    let mut rng = Rng::new(777);
    for &alphabet in &[2u8, 5, 60] {
        for len in [1usize, 2, 63, 64, 65, 300, 2000] {
            let text = random_bytes(&mut rng, len, alphabet);
            let fm = FmIndex::build(&text);
            // 全区間(短い入力)または無作為な区間(長い入力)
            if len <= 300 {
                for a in 0..len {
                    for b in a..=len {
                        assert_eq!(
                            fm.extract(a, b - a),
                            text[a..b],
                            "alphabet={alphabet} len={len} [{a},{b})"
                        );
                    }
                }
            } else {
                for _ in 0..400 {
                    let a = rng.below(len as u64) as usize;
                    let l = rng.below((len - a) as u64 + 1) as usize;
                    assert_eq!(
                        fm.extract(a, l),
                        text[a..a + l],
                        "len={len} [{a},{})",
                        a + l
                    );
                }
            }
        }
    }
}

#[test]
fn o_11b_日本語の実文で復元できる() {
    let text = "春はあけぼの。やうやう白くなりゆく山ぎは、すこしあかりて、\
                 むらさきだちたる雲のほそくたなびきたる。\
                 夏は夜。月のころはさらなり、闇もなほ、蛍の多く飛びちがひたる。\
                 秋は夕暮。夕日のさして山の端いと近うなりたるに、\
                 烏の寝どころへ行くとて、三つ四つ、二つ三つなど飛びいそぐさへあはれなり。";
    let b = text.as_bytes();
    let fm = FmIndex::build(b);
    assert_eq!(fm.extract(0, b.len()), b, "全文の復元");
    for a in 0..b.len() {
        for l in [1usize, 7, 40] {
            let hi = (a + l).min(b.len());
            assert_eq!(fm.extract(a, hi - a), &b[a..hi], "[{a},{hi})");
        }
    }
}

#[test]
fn o_11c_文脈が文字境界で切れて作品をはみ出さない() {
    let a = "春はあけぼの。やうやう白くなりゆく山ぎは、すこしあかりて。";
    let b = "秋は夕暮。烏の寝どころへ行くとて、飛びいそぐさへあはれなり。";
    let text = format!("{a}{b}");
    let bytes = text.as_bytes();
    let fm = FmIndex::build(bytes);
    let pat = "は".as_bytes();
    for &pos in &fm.locate(pat) {
        // 位置が属する作品の範囲
        let bounds = if pos < a.len() {
            (0, a.len())
        } else {
            (a.len(), bytes.len())
        };
        let (before, hit, after) = fm.kwic(pos, pat.len(), 12, bounds);
        assert_eq!(hit, pat, "一致部が原文と違う");
        // すべて正しい UTF-8 に切れている
        for (part, name) in [(&before, "前文脈"), (&after, "後文脈")] {
            assert!(
                std::str::from_utf8(part).is_ok(),
                "{name} が文字境界で切れていない: {part:?}"
            );
        }
        // 作品をはみ出していない
        let whole = [before.clone(), hit.clone(), after.clone()].concat();
        let doc = &bytes[bounds.0..bounds.1];
        assert!(
            doc.windows(whole.len()).any(|w| w == whole),
            "文脈が作品 [{},{}) の外にはみ出した",
            bounds.0,
            bounds.1
        );
    }
}

// ---------------------------------------------------------------- O-12

#[test]
fn o_12_配信形式の指紋が版と対応している() {
    // 形式の並びを変えると指紋が変わる。このゲートが落ちたら、
    // **先に VERSION を上げてから**指紋を更新すること。
    // 版を上げ忘れると旧形式のファイルが検査を通過して誤読される(実際に起きた)。
    use aozora_sakuin::shard::{format_fingerprint, VERSION};
    assert_eq!(VERSION, 2, "版が変わった — 指紋も更新すること");
    assert_eq!(
        format_fingerprint(),
        0x0669c68dd6379501,
        "配信形式の並びが変わっている。VERSION を上げてから指紋を更新すること"
    );
}
