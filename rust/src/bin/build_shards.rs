//! 配信用シャードを組む(SPEC F-03 / F-08)。
//!
//!   cargo run --release --bin build_shards -- [1 シャードの本文目標バイト数]
//!
//! 出力: web/index/shard-NNN.azsk と web/index/manifest.json

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use aozora_sakuin::fm::find_all_naive;
use aozora_sakuin::shard::{plan_shards, Doc, Shard};

const DEFAULT_TARGET: usize = 6_000_000;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let src = root.join("data/normalized");
    let out = root.join("web/index");
    let target: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.replace('_', "").parse().ok())
        .unwrap_or(DEFAULT_TARGET);

    let mut files: Vec<PathBuf> = fs::read_dir(&src)
        .unwrap_or_else(|e| panic!("{} を読めない: {e}", src.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "txt").unwrap_or(false))
        .collect();
    files.sort();

    println!("正規化本文 {} 作を読み込む…", files.len());
    let t0 = Instant::now();
    let texts: Vec<(u32, Vec<u8>)> = files
        .iter()
        .map(|f| {
            let id: u32 = f.file_stem().unwrap().to_str().unwrap().parse().unwrap();
            (id, fs::read(f).unwrap())
        })
        .collect();
    let sizes: Vec<usize> = texts.iter().map(|(_, t)| t.len()).collect();
    let total: usize = sizes.iter().sum();
    println!(
        "  {} 作 / {:.1} MB / {:.1}s",
        texts.len(),
        total as f64 / 1e6,
        t0.elapsed().as_secs_f64()
    );

    let plan = plan_shards(&sizes, target);
    println!(
        "\nシャード {} 枚(本文目標 {:.1} MB/枚)",
        plan.len(),
        target as f64 / 1e6
    );

    fs::create_dir_all(&out).unwrap();
    let t1 = Instant::now();
    let mut manifest = Vec::new();
    let (mut sum_bytes, mut sum_resident, mut max_bytes) = (0usize, 0usize, 0usize);

    for (k, &(a, b)) in plan.iter().enumerate() {
        let mut text = Vec::new();
        let mut docs = Vec::new();
        for (id, t) in &texts[a..b] {
            docs.push(Doc {
                id: *id,
                offset: text.len() as u32,
            });
            text.extend_from_slice(t);
        }
        let shard = Shard::build(&text, docs);
        let bytes = shard.to_bytes();
        let resident = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;

        let name = format!("shard-{k:03}.azsk");
        fs::write(out.join(&name), &bytes).unwrap();
        sum_bytes += bytes.len();
        sum_resident += resident;
        max_bytes = max_bytes.max(bytes.len());
        manifest.push((
            name,
            b - a,
            text.len(),
            bytes.len(),
            resident,
            texts[a].0,
            texts[b - 1].0,
        ));
        if k % 10 == 0 || k + 1 == plan.len() {
            println!(
                "  [{k:3}/{}] {} 作 / 本文 {:.1} MB → 索引 {:.1} MB(常駐 {:.2} MB) / 経過 {:.0}s",
                plan.len(),
                b - a,
                text.len() as f64 / 1e6,
                bytes.len() as f64 / 1e6,
                resident as f64 / 1e6,
                t1.elapsed().as_secs_f64()
            );
        }
    }

    let mut js = format!(
        "{{\n  \"format_version\": {},\n  \"shards\": [\n",
        aozora_sakuin::shard::VERSION
    );
    for (i, (name, docs, text_len, len, resident, first, last)) in manifest.iter().enumerate() {
        js.push_str(&format!(
            "    {{\"file\": \"{name}\", \"docs\": {docs}, \"text\": {text_len}, \
             \"bytes\": {len}, \"resident\": {resident}, \"first_id\": {first}, \"last_id\": {last}}}{}\n",
            if i + 1 == manifest.len() { "" } else { "," }
        ));
    }
    js.push_str(&format!(
        "  ],\n  \"total_text\": {total},\n  \"total_bytes\": {sum_bytes},\n  \
         \"total_resident\": {sum_resident},\n  \"works\": {}\n}}\n",
        texts.len()
    ));
    fs::write(out.join("manifest.json"), js).unwrap();

    println!("\n--- 配信の実測 ---");
    println!(
        "シャード      {} 枚 / 最大 {:.1} MB",
        plan.len(),
        max_bytes as f64 / 1e6
    );
    println!(
        "索引 総計     {:.1} MB(本文 {:.1} MB の {:.3} 倍)",
        sum_bytes as f64 / 1e6,
        total as f64 / 1e6,
        sum_bytes as f64 / total as f64
    );
    println!(
        "常駐領域 総計 {:.1} MB(全シャードの先頭部分・初回だけ落とす)",
        sum_resident as f64 / 1e6
    );
    println!(
        "表示 1 回あたり 最大 {:.1} MB(該当シャードを丸ごと)",
        max_bytes as f64 / 1e6
    );
    println!("構築          {:.0}s", t1.elapsed().as_secs_f64());

    // --- 横断照合(O-10): シャード合計 = 全文の総当たり ---
    println!("\n--- 横断照合 ---");
    let whole: Vec<u8> = texts.iter().flat_map(|(_, t)| t.clone()).collect();
    let mut shards: Vec<Shard> = Vec::new();
    for (name, ..) in &manifest {
        shards.push(Shard::from_bytes(&fs::read(out.join(name)).unwrap()).unwrap());
    }
    for word in ["あはれ", "うつくしい", "東京", "吾輩"] {
        let p = word.as_bytes();
        let sum: usize = shards.iter().map(|s| s.fm.count(p)).sum();
        let want = find_all_naive(&whole, p).len();
        // 作品はシャードをまたがないので、境界で分断されるヒットは無い
        println!(
            "  {word:<10} シャード合計 {sum:>8} / 全文の総当たり {want:>8}   {}",
            if sum == want {
                "一致"
            } else {
                "★不一致★"
            }
        );
        assert_eq!(sum, want, "{word} でシャード合計が総当たりと一致しない");
    }

    // 作品別の件数も引けること
    let mut by_work: BTreeMap<u32, usize> = BTreeMap::new();
    for s in &shards {
        for &pos in &s.fm.locate("吾輩".as_bytes()) {
            *by_work.entry(s.docs[s.doc_of(pos)].id).or_default() += 1;
        }
    }
    let top: Vec<_> = {
        let mut v: Vec<_> = by_work.iter().collect();
        v.sort_by_key(|(_, &n)| std::cmp::Reverse(n));
        v.into_iter().take(5).collect()
    };
    println!("\n「吾輩」が多い作品 ID: {top:?}");
}
