//! 表記ゆれの展開を実データで確かめる。
//!
//!   cargo run --release --bin variants_demo -- <語>...

use std::fs;
use std::path::PathBuf;

use aozora_sakuin::shard::Shard;
use aozora_sakuin::variants::expand;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let dir = root.join("web/index");
    let words: Vec<String> = {
        let a: Vec<String> = std::env::args().skip(1).collect();
        if a.is_empty() {
            [
                "あわれ",
                "おもう",
                "ように",
                "そうして",
                "しずか",
                "こえ",
                "おんな",
            ]
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
    let shards: Vec<Shard> = files
        .iter()
        .map(|f| Shard::from_bytes(&fs::read(f).unwrap()).unwrap())
        .collect();

    let count = |w: &str| -> usize { shards.iter().map(|s| s.fm.count(w.as_bytes())).sum() };

    for w in &words {
        let base = count(w);
        let forms = expand(w, false);
        let risky: Vec<String> = expand(w, true)
            .into_iter()
            .filter(|f| !forms.contains(f))
            .collect();
        let mut extra = 0usize;
        println!("\n「{w}」 素の件数 {base}");
        for f in &forms {
            let c = count(f);
            if c > 0 {
                extra += c;
                println!("   + {f:<12} {c:>8}");
            }
        }
        println!(
            "   → 既定で {} 件({:+.0}%)",
            base + extra,
            if base > 0 {
                extra as f64 / base as f64 * 100.0
            } else {
                0.0
            }
        );
        let mut rc = 0usize;
        for f in &risky {
            let c = count(f);
            if c > 0 {
                rc += c;
                println!("   ? {f:<12} {c:>8}  (衝突しやすい規則)");
            }
        }
        if rc > 0 {
            println!("   → 衝突しやすい規則も入れると {} 件", base + extra + rc);
        }
    }
}
