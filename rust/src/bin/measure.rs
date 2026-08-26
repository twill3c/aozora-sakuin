//! 索引サイズと構築時間を実データで測る。
//!
//!   cargo run --release --bin measure -- [上限バイト数]
//!
//! 見積もりではなく実測を SPEC に書き戻すためのもの。出荷経路ではない。

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use aozora_sakuin::fm::{find_all_naive, FmIndex, SA_SAMPLE};

/// 作品の境目。本文(UTF-8)には現れないバイト
const DOC_SEP: u8 = 0x01;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let dir = root.join("data/normalized");
    let limit: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.replace('_', "").parse().ok())
        .unwrap_or(usize::MAX);

    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} を読めない: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "txt").unwrap_or(false))
        .collect();
    files.sort();

    let t0 = Instant::now();
    let mut text: Vec<u8> = Vec::new();
    let mut docs = 0usize;
    for f in &files {
        if text.len() >= limit {
            break;
        }
        let b = fs::read(f).unwrap();
        text.extend_from_slice(&b);
        text.push(DOC_SEP);
        docs += 1;
    }
    text.truncate(limit.min(text.len()));
    // 末尾が途中で切れていても番兵の条件(0 を含まない)は保たれる
    assert!(!text.contains(&0), "本文に NUL バイトが含まれている");
    let load = t0.elapsed();

    println!(
        "本文  {docs} 作 / {} バイト ({:.1} MB) / 読み込み {:.1}s",
        text.len(),
        text.len() as f64 / 1e6,
        load.as_secs_f64()
    );

    let t1 = Instant::now();
    let fm = FmIndex::build(&text);
    let build = t1.elapsed();

    let size = fm.size_bytes();
    println!(
        "索引  {} バイト ({:.1} MB) / 原文比 {:.3} 倍 / 構築 {:.1}s (SA 標本間隔 {SA_SAMPLE})",
        size,
        size as f64 / 1e6,
        size as f64 / text.len() as f64,
        build.as_secs_f64()
    );
    println!(
        "      平均符号長 {:.2} ビット/バイト",
        fm.mean_code_len(&text)
    );
    if std::env::var("SKIP_STEPS").is_err() {
        let t = Instant::now();
        let steps = fm.max_locate_steps();
        println!(
            "      位置復元の歩数 最大 {} 歩(全 {} 行を走査 / {:.1}s)",
            steps,
            text.len() + 1,
            t.elapsed().as_secs_f64()
        );
    }
    println!(
        "      全 5,000 作(正味 283.7 MB)なら索引 {:.0} MB",
        283.7 * size as f64 / text.len() as f64
    );

    // 探索の実測。総当たりと突き合わせて、測っているものが正しいことも同時に確かめる
    println!("\n語        件数      検索      位置復元(全件)   総当たり照合");
    for word in ["あはれ", "うつくしい", "東京", "こころ", "の", "吾輩"] {
        let p = word.as_bytes();
        let t = Instant::now();
        let n = fm.count(p);
        let t_count = t.elapsed();
        let t = Instant::now();
        let pos = fm.locate(p);
        let t_locate = t.elapsed();
        let want = find_all_naive(&text, p);
        let ok = pos == want;
        println!(
            "{word:<10}{n:>7}  {:>8.3}ms  {:>10.1}ms      {}",
            t_count.as_secs_f64() * 1e3,
            t_locate.as_secs_f64() * 1e3,
            if ok { "一致" } else { "★不一致★" }
        );
        assert!(ok, "{word} で総当たりと不一致");
    }
}
