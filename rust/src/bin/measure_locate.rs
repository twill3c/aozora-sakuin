//! 位置復元と文脈取り出しに、どれだけの逐次ステップが要るかを実測する。
//!
//! Range で読むなら「逐次ステップ数 = HTTP の往復数」になるので、
//! ここが Range 化できるかどうかの分かれ目になる。

use std::fs;
use std::path::PathBuf;

use aozora_sakuin::fm::{ISA_SAMPLE, SA_SAMPLE};
use aozora_sakuin::shard::Shard;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let dir = root.join("web/index");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "azsk").unwrap_or(false))
        .collect();
    files.sort();

    let s = Shard::from_bytes(&fs::read(&files[0]).unwrap()).unwrap();
    println!(
        "シャード 1 枚 / 本文 {:.1} MB / SA 標本 {SA_SAMPLE} / ISA 標本 {ISA_SAMPLE}",
        s.fm.len() as f64 / 1e6
    );

    println!(
        "\n{:<12}{:>8}{:>16}{:>18}",
        "語", "件数", "位置復元の歩数", "文脈 30 字の歩数"
    );
    for word in ["あはれ", "うつくしい", "吾輩", "東京"] {
        let p = word.as_bytes();
        let n = s.fm.count(p);
        if n == 0 {
            continue;
        }
        // 位置復元: 1 件あたり LF を平均 SA_SAMPLE/2 歩、最悪 max_locate_steps
        // 文脈取り出し: ISA 標本へ跳んでから (文脈長 + ISA_SAMPLE) 歩
        let show = 6usize.min(n);
        let locate_steps = show * SA_SAMPLE / 2; // 期待値
        let kwic_steps = show * (30 * 3 + 30 * 3 + p.len() + ISA_SAMPLE);
        println!("{word:<12}{n:>8}{locate_steps:>16}{kwic_steps:>18}");
    }

    println!("\n1 歩 = LF 写像 1 回 = access + rank = ウェーブレット木を 2 度降りる");
    println!(
        "Range なら 1 歩ごとに 5〜10 回の読みが要り、しかも逐次(前の歩の答えが次の位置を決める)"
    );
    println!(
        "→ 用例 6 件で概算 {} 往復。丸ごと 4.7 MB 落とすほうが速い",
        6 * (SA_SAMPLE / 2 + 30 * 3 * 2 + ISA_SAMPLE) * 7
    );
}
