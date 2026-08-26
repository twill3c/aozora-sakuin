//! Range で必要なバイトだけ読む経路のオラクル(SPEC O-03)。
//!
//! **丸ごと読んだ索引と、必要な分だけ供給した索引が、同じ答えを返すこと。**
//! 二つの経路を別々に実装しているので、これは二実装照合になる。

use aozora_sakuin::fm::find_all_naive;
use aozora_sakuin::shard::{Doc, PartialShard, Shard};

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

/// 常駐領域だけを渡して組み、要求されたバイトだけを供給しながら数える。
/// 返すのは (件数, 往復回数, 供給したバイト数)
fn count_over_range(bytes: &[u8], pattern: &[u8]) -> (usize, usize, usize) {
    let resident = PartialShard::resident_len(bytes).expect("ヘッダを読めない");
    let mut ps = PartialShard::from_resident(&bytes[..resident]).expect("常駐領域から組めない");
    let (mut rounds, mut fed) = (0usize, 0usize);
    loop {
        match ps.try_count(pattern) {
            Ok(n) => return (n, rounds, fed),
            Err(ranges) => {
                assert!(!ranges.is_empty(), "足りないのに要求が空");
                rounds += 1;
                assert!(rounds < 10_000, "収束しない");
                for r in ranges {
                    let a = r.at as usize;
                    let b = (a + r.len as usize).min(bytes.len());
                    ps.supply(r.at, &bytes[a..b]);
                    fed += b - a;
                }
            }
        }
    }
}

#[test]
fn o_03_必要な分だけ読んでも丸ごと読んだのと同じ件数になる() {
    let mut rng = Rng::new(20_260_827);
    let mut checked = 0usize;
    for &alphabet in &[2u8, 5, 60] {
        for len in [200usize, 3000, 20_000] {
            let text = random_bytes(&mut rng, len, alphabet);
            let shard = Shard::build(&text, vec![Doc { id: 1, offset: 0 }]);
            let bytes = shard.to_bytes();
            for plen in 1..=5usize.min(len) {
                for _ in 0..6 {
                    let at = rng.below((len - plen + 1) as u64) as usize;
                    let pat = &text[at..at + plen];
                    let want = find_all_naive(&text, pat).len();
                    let whole = shard.fm.count(pat);
                    let (got, _, _) = count_over_range(&bytes, pat);
                    assert_eq!(whole, want, "丸ごと読みが総当たりと違う");
                    assert_eq!(got, want, "部分読みが総当たりと違う pat={pat:?}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked >= 200, "検査した組が {checked} 件しかない");
}

#[test]
fn o_03b_見つからない語でも一致する() {
    let mut rng = Rng::new(31);
    let text = random_bytes(&mut rng, 5000, 4);
    let bytes = Shard::build(&text, vec![Doc { id: 1, offset: 0 }]).to_bytes();
    for pat in [vec![250u8], vec![250, 251], vec![1, 250, 2]] {
        let want = find_all_naive(&text, &pat).len();
        let (got, _, _) = count_over_range(&bytes, &pat);
        assert_eq!(got, want, "pat={pat:?}");
    }
}

#[test]
fn o_03c_日本語の実文で一致し転送が桁違いに少ない() {
    let text = "春はあけぼの。やうやう白くなりゆく山ぎは、すこしあかりて、\
                 むらさきだちたる雲のほそくたなびきたる。\
                 夏は夜。月のころはさらなり、闇もなほ、蛍の多く飛びちがひたる。\
                 秋は夕暮。夕日のさして山の端いと近うなりたるに、\
                 烏の寝どころへ行くとて、三つ四つ、二つ三つなど飛びいそぐさへあはれなり。\
                 冬はつとめて。雪の降りたるはいふべきにもあらず。"
        .repeat(40);
    let b = text.as_bytes();
    let shard = Shard::build(b, vec![Doc { id: 1, offset: 0 }]);
    let bytes = shard.to_bytes();
    let resident = PartialShard::resident_len(&bytes).unwrap();
    for pat in ["あはれ", "やうやう", "夕暮", "見当"] {
        let p = pat.as_bytes();
        let want = find_all_naive(b, p).len();
        let (got, rounds, fed) = count_over_range(&bytes, p);
        assert_eq!(got, want, "pat={pat}");
        // 往復は「語のバイト数 × 木の深さ」程度で収まる
        assert!(rounds <= p.len() * 12, "pat={pat} で {rounds} 往復");
        // 供給したバイトは索引全体よりずっと小さい
        assert!(
            resident + fed < bytes.len(),
            "pat={pat} で常駐 {resident} + 供給 {fed} が全体 {} を下回らない",
            bytes.len()
        );
    }
}

#[test]
fn o_03d_常駐領域は索引全体の一割未満である() {
    let mut rng = Rng::new(7);
    let text = random_bytes(&mut rng, 200_000, 60);
    let bytes = Shard::build(&text, vec![Doc { id: 1, offset: 0 }]).to_bytes();
    let resident = PartialShard::resident_len(&bytes).unwrap();
    let ratio = resident as f64 / bytes.len() as f64;
    assert!(ratio < 0.10, "常駐領域が全体の {:.1}%", ratio * 100.0);
}
