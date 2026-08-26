//! 索引だけから KWIC を組んで見せる(SPEC F-05 の実地確認)。
//!
//!   cargo run --release --bin kwic_demo -- <語> [表示件数]

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use aozora_sakuin::shard::Shard;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let dir = root.join("web/index");
    let word = std::env::args().nth(1).unwrap_or_else(|| "あはれ".into());
    let show: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let pat = word.as_bytes();

    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "azsk").unwrap_or(false))
        .collect();
    files.sort();

    // 第 1 段: 件数だけ数える(全シャード)
    let t0 = Instant::now();
    let mut counts = Vec::new();
    let mut total = 0usize;
    let mut loaded = Vec::new();
    for f in &files {
        let s = Shard::from_bytes(&fs::read(f).unwrap()).unwrap();
        let n = s.fm.count(pat);
        total += n;
        counts.push(n);
        loaded.push(s);
    }
    println!(
        "「{word}」 {total} 件 / シャード {} 枚 / 読み込み+計数 {:.2}s",
        files.len(),
        t0.elapsed().as_secs_f64()
    );

    // 第 2 段: 件のあるシャードから、表示する分だけ位置と文脈を取り出す
    let t1 = Instant::now();
    let mut shown = 0usize;
    println!();
    for (k, s) in loaded.iter().enumerate() {
        if counts[k] == 0 || shown >= show {
            continue;
        }
        for &pos in s.fm.locate(pat).iter() {
            if shown >= show {
                break;
            }
            let d = s.doc_of(pos);
            let lo = s.docs[d].offset as usize;
            let hi = s
                .docs
                .get(d + 1)
                .map(|x| x.offset as usize)
                .unwrap_or(s.fm.len());
            let (b, h, a) = s.fm.kwic(pos, pat.len(), 30, (lo, hi));
            println!(
                "  …{}【{}】{}…",
                String::from_utf8_lossy(&b),
                String::from_utf8_lossy(&h),
                String::from_utf8_lossy(&a)
            );
            println!("     作品 {} / 作品内 {} 字目\n", s.docs[d].id, pos - lo);
            shown += 1;
        }
    }
    println!(
        "文脈の取り出し {:.1}ms({} 件)",
        t1.elapsed().as_secs_f64() * 1e3,
        shown
    );
}
