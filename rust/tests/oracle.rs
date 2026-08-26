//! 二実装照合のオラクル(SPEC O-01 / O-02)。
//!
//! SA-IS と BWT の正しさは、総当たり参照実装との一致で担保する。
//! 圧縮索引が自己無矛盾であることは解釈の正しさを意味しない(HC-027)ため、
//! 逆変換の往復だけを根拠にしてはならない。

use aozora_sakuin::bwt;
use aozora_sakuin::sais;

/// 決定論的な擬似乱数(xorshift64*)。外部クレートを持ち込まないため自前で持つ。
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
        self.next() % n
    }
}

/// 0 を含まないランダムなバイト列
fn random_bytes(rng: &mut Rng, len: usize, alphabet: u8) -> Vec<u8> {
    (0..len)
        .map(|_| 1 + rng.below(alphabet as u64) as u8)
        .collect()
}

// ---------------------------------------------------------------- O-SA

#[test]
fn o_sa_01_無作為な入力で総当たりと一致する() {
    let mut rng = Rng::new(20_260_826);
    let mut cases = 0;
    for &alphabet in &[1u8, 2, 3, 5, 26, 255] {
        for len in 0..=120 {
            for _ in 0..3 {
                let s = random_bytes(&mut rng, len, alphabet);
                assert_eq!(
                    sais::suffix_array(&s),
                    sais::suffix_array_naive(&s),
                    "alphabet={alphabet} len={len} s={s:?}"
                );
                cases += 1;
            }
        }
    }
    assert!(cases >= 2000, "検査した入力が {cases} 件しかない");
}

#[test]
fn o_sa_02_退化した入力でも一致する() {
    let mut cases: Vec<Vec<u8>> = vec![
        b"".to_vec(),
        b"a".to_vec(),
        b"aa".to_vec(),
        b"ab".to_vec(),
        b"ba".to_vec(),
        vec![7u8; 300],
        b"abababababababab".to_vec(),
        b"mississippi".to_vec(),
        b"banana".to_vec(),
        b"aaaaabaaaaabaaaaab".to_vec(),
    ];
    // フィボナッチ語 — LMS の入れ子が深くなり再帰段数が伸びる
    let (mut a, mut b) = (b"a".to_vec(), b"ab".to_vec());
    for _ in 0..12 {
        let next = [b.clone(), a].concat();
        a = b;
        b = next;
    }
    cases.push(b);
    for s in cases {
        assert_eq!(
            sais::suffix_array(&s),
            sais::suffix_array_naive(&s),
            "len={}",
            s.len()
        );
    }
}

#[test]
fn o_sa_03_日本語の実文で一致する() {
    let text = "春はあけぼの。やうやう白くなりゆく山ぎは、すこしあかりて、\
                 むらさきだちたる雲のほそくたなびきたる。\
                 ゆく河の流れは絶えずして、しかももとの水にあらず。\
                 つれづれなるままに、日暮らし硯にむかひて。";
    let s = text.as_bytes();
    assert_eq!(sais::suffix_array(s), sais::suffix_array_naive(s));
}

#[test]
fn o_sa_04_接尾辞配列は順列であり整列している() {
    let mut rng = Rng::new(7);
    for len in [0usize, 1, 2, 50, 500, 3000] {
        let s = random_bytes(&mut rng, len, 4);
        let sa = sais::suffix_array(&s);
        assert_eq!(sa.len(), len);
        let mut seen = vec![false; len];
        for &i in &sa {
            assert!(!seen[i as usize], "位置 {i} が重複している");
            seen[i as usize] = true;
        }
        for w in sa.windows(2) {
            assert!(
                s[w[0] as usize..] < s[w[1] as usize..],
                "整列していない: {} と {}",
                w[0],
                w[1]
            );
        }
    }
}

// ---------------------------------------------------------------- O-BWT

#[test]
fn o_bwt_01_無作為な入力で総当たりと一致する() {
    let mut rng = Rng::new(31_415_926);
    for &alphabet in &[2u8, 3, 26] {
        for len in 1..=60 {
            let s = random_bytes(&mut rng, len, alphabet);
            let b = bwt::transform(&s);
            assert_eq!(
                b.last,
                bwt::transform_naive(&s),
                "alphabet={alphabet} len={len} s={s:?}"
            );
        }
    }
}

#[test]
fn o_bwt_02_逆変換が原文に戻る() {
    let mut rng = Rng::new(2_718_281);
    for &alphabet in &[1u8, 2, 5, 200] {
        for len in 1..=200 {
            let s = random_bytes(&mut rng, len, alphabet);
            let b = bwt::transform(&s);
            assert_eq!(bwt::inverse(&b), s, "alphabet={alphabet} len={len}");
        }
    }
}

#[test]
fn o_bwt_03_日本語の実文で往復する() {
    let text = "吾輩は猫である。名前はまだ無い。\
                 どこで生れたかとんと見当がつかぬ。\
                 何でも薄暗いじめじめした所でニャーニャー泣いていた事だけは記憶している。";
    let s = text.as_bytes();
    let b = bwt::transform(s);
    assert_eq!(b.last, bwt::transform_naive(s));
    assert_eq!(bwt::inverse(&b), s);
    assert_eq!(String::from_utf8(bwt::inverse(&b)).unwrap(), text);
}
