//! Range で必要な分だけ読んだときの転送量と往復回数を実測する(SPEC O-03)。
//!
//!   cargo run --release --bin measure_range -- [語]...

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use aozora_sakuin::shard::{PartialShard, Shard};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let dir = root.join("web/index");
    let words: Vec<String> = {
        let a: Vec<String> = std::env::args().skip(1).collect();
        if a.is_empty() {
            ["あはれ", "うつくしい", "吾輩", "東京", "の"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            a
        }
    };

    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "azsk").unwrap_or(false))
        .collect();
    files.sort();

    // ファイルの中身は「サーバに置いてある」ものとして、読むのは要求した範囲だけと数える
    let blobs: Vec<Vec<u8>> = files.iter().map(|f| fs::read(f).unwrap()).collect();
    let whole: usize = blobs.iter().map(|b| b.len()).sum();
    let resident: usize = blobs
        .iter()
        .map(|b| PartialShard::resident_len(b).unwrap())
        .sum();
    println!(
        "シャード {} 枚 / 索引 {:.1} MB / 常駐領域 {:.1} MB({:.1}%)",
        blobs.len(),
        whole as f64 / 1e6,
        resident as f64 / 1e6,
        resident as f64 / whole as f64 * 100.0
    );

    println!(
        "\n{:<12}{:>10}{:>12}{:>12}{:>10}{:>12}",
        "語", "件数", "往復(最大)", "追加転送", "総転送", "丸ごと比"
    );
    for w in &words {
        let pat = w.as_bytes();
        let t0 = Instant::now();
        let (mut total, mut fed, mut max_rounds) = (0usize, 0usize, 0usize);
        let mut check = 0usize;
        for b in &blobs {
            let r = PartialShard::resident_len(b).unwrap();
            let mut ps = PartialShard::from_resident(&b[..r]).unwrap();
            let mut rounds = 0usize;
            loop {
                match ps.try_count(pat) {
                    Ok(n) => {
                        total += n;
                        break;
                    }
                    Err(ranges) => {
                        rounds += 1;
                        for x in ranges {
                            let a = x.at as usize;
                            let e = (a + x.len as usize).min(b.len());
                            ps.supply(x.at, &b[a..e]);
                            fed += e - a;
                        }
                    }
                }
            }
            max_rounds = max_rounds.max(rounds);
            // 丸ごと読んだ場合と一致することを同時に確かめる
            check += Shard::from_bytes(b).unwrap().fm.count(pat);
        }
        assert_eq!(total, check, "「{w}」で部分読みと丸ごと読みが一致しない");
        let sent = resident + fed;
        println!(
            "{:<12}{:>10}{:>12}{:>11.2}MB{:>9.1}MB{:>11.1}x  {:.1}s",
            w,
            total,
            max_rounds,
            fed as f64 / 1e6,
            sent as f64 / 1e6,
            whole as f64 / sent as f64,
            t0.elapsed().as_secs_f64()
        );
    }
    println!("\n※ 総転送 = 常駐領域(初回のみ)+ 追加転送。2 語目以降は追加転送だけで済む");
}
