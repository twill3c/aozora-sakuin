//! wasm 境界のオラクル(SPEC F-04 / O-13)。
//!
//! 境界の向こうから見える結果が、メモリ上の索引および総当たり探索と一致すること。
//! ABI は素の Rust 関数なので、wasm に載せる前にここで全て検査できる。

use std::sync::Mutex;

use aozora_sakuin::fm::find_all_naive;
use aozora_sakuin::shard::{Doc, Shard};
use aozora_sakuin::wasm::*;

/// 境界は `static mut` のシャード登録簿と出力バッファを持つ。wasm は単一スレッドなので
/// それで正しいが、`cargo test` は既定で並列に走るので**テストの側で**直列化する。
/// (nanpure-forge で同じ型を踏んでいる)
static SERIAL: Mutex<()> = Mutex::new(());

/// JS が読むのと同じ手順でレコード列をほどく。
/// この関数が「JS 側が知っていればよいこと」の全てになる。
fn decode(buf: &[u8]) -> Vec<(u32, u32, String, String, String)> {
    let mut out = Vec::new();
    let mut p = 0usize;
    let u32at = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    while p < buf.len() {
        let id = u32at(buf, p);
        let pos = u32at(buf, p + 4);
        let (bl, hl, al) = (
            u32at(buf, p + 8) as usize,
            u32at(buf, p + 12) as usize,
            u32at(buf, p + 16) as usize,
        );
        p += 20;
        let s = |a: usize, b: usize| String::from_utf8(buf[a..b].to_vec()).expect("不正な UTF-8");
        out.push((
            id,
            pos,
            s(p, p + bl),
            s(p + bl, p + bl + hl),
            s(p + bl + hl, p + bl + hl + al),
        ));
        p += bl + hl + al;
    }
    out
}

fn load(text: &[u8], docs: Vec<Doc>) -> (i32, Vec<u8>) {
    let bytes = Shard::build(text, docs).to_bytes();
    let h = unsafe {
        let p = az_alloc(bytes.len() as u32);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        az_shard_load(p, bytes.len() as u32)
    };
    assert!(h >= 0, "シャードを読み込めない");
    (h, bytes)
}

#[test]
fn o_13_境界越しの件数が総当たりと一致する() {
    let _g = SERIAL.lock().unwrap();
    let a = "春はあけぼの。やうやう白くなりゆく山ぎは、すこしあかりて。";
    let b = "秋は夕暮。烏の寝どころへ行くとて、飛びいそぐさへあはれなり。";
    let text = format!("{a}{b}");
    let bytes = text.as_bytes();
    let (h, _keep) = load(
        bytes,
        vec![
            Doc { id: 11, offset: 0 },
            Doc {
                id: 22,
                offset: a.len() as u32,
            },
        ],
    );
    unsafe {
        assert_eq!(az_shard_docs(h), 2);
        assert_eq!(az_format_version(), 2);
        for pat in ["は", "あけぼの", "夕暮", "あはれ", "ゐ", "。"] {
            let p = pat.as_bytes();
            let got = az_count(h, p.as_ptr(), p.len() as u32);
            assert_eq!(got as usize, find_all_naive(bytes, p).len(), "pat={pat}");
        }
        // 無効な取っ手はエラーを返す(パニックしない)
        assert_eq!(az_count(999, "は".as_bytes().as_ptr(), 3), -1);
        assert_eq!(az_shard_docs(-1), -1);
    }
}

#[test]
fn o_13b_境界越しの用例が原文と一致する() {
    let _g = SERIAL.lock().unwrap();
    let a = "春はあけぼの。やうやう白くなりゆく山ぎは、すこしあかりて。";
    let b = "秋は夕暮。烏の寝どころへ行くとて、飛びいそぐさへあはれなり。";
    let text = format!("{a}{b}");
    let bytes = text.as_bytes();
    let (h, _keep) = load(
        bytes,
        vec![
            Doc { id: 11, offset: 0 },
            Doc {
                id: 22,
                offset: a.len() as u32,
            },
        ],
    );
    unsafe {
        for pat in ["は", "夕暮", "あはれ"] {
            let p = pat.as_bytes();
            let n = az_kwic(h, p.as_ptr(), p.len() as u32, 12, 50);
            assert!(n >= 0, "pat={pat} で失敗");
            let out = core::slice::from_raw_parts(az_out_ptr(), az_out_len() as usize);
            let recs = decode(out);
            assert_eq!(recs.len(), n as usize);
            assert_eq!(
                recs.len(),
                find_all_naive(bytes, p).len(),
                "pat={pat} の件数"
            );
            for (id, pos, before, hit, after) in &recs {
                assert_eq!(hit, pat, "一致部が違う");
                // 作品と作品内位置が正しい
                let doc = if *id == 11 { a } else { b };
                assert_eq!(
                    &doc.as_bytes()[*pos as usize..*pos as usize + p.len()],
                    p,
                    "作品 {id} の {pos} 字目が一致部でない"
                );
                // 前後文脈が原文にそのまま現れる
                let whole = format!("{before}{hit}{after}");
                assert!(
                    doc.contains(&whole),
                    "文脈が作品 {id} の本文にない: {whole}"
                );
            }
        }
    }
}

#[test]
fn o_13c_取っ手を解放しても他のシャードは生きている() {
    let _g = SERIAL.lock().unwrap();
    let (h1, _k1) = load("あはれなり".as_bytes(), vec![Doc { id: 1, offset: 0 }]);
    let (h2, _k2) = load("うつくしきもの".as_bytes(), vec![Doc { id: 2, offset: 0 }]);
    unsafe {
        az_shard_drop(h1);
        assert_eq!(
            az_count(h1, "あはれ".as_bytes().as_ptr(), 9),
            -1,
            "解放後は使えない"
        );
        let p = "うつくし".as_bytes();
        assert_eq!(
            az_count(h2, p.as_ptr(), p.len() as u32),
            1,
            "他のシャードは無事"
        );
        az_shard_drop(h2);
    }
}

#[test]
fn o_13d_max件で打ち切れる() {
    let _g = SERIAL.lock().unwrap();
    let text = "はははははははははは";
    let (h, _keep) = load(text.as_bytes(), vec![Doc { id: 1, offset: 0 }]);
    unsafe {
        let p = "は".as_bytes();
        assert_eq!(az_count(h, p.as_ptr(), 3), 10);
        for max in [0u32, 1, 3, 10, 99] {
            let n = az_kwic(h, p.as_ptr(), 3, 6, max);
            assert_eq!(n as u32, max.min(10), "max={max}");
            let out = core::slice::from_raw_parts(az_out_ptr(), az_out_len() as usize);
            assert_eq!(decode(out).len(), n as usize);
        }
        az_shard_drop(h);
    }
}

#[test]
fn o_13e_境界越しに異体形を取り出せる() {
    let _g = SERIAL.lock().unwrap();
    unsafe {
        for (q, want) in [("あわれ", "あはれ"), ("ように", "やうに"), ("いる", "ゐる")]
        {
            let b = q.as_bytes();
            let n = az_variants(b.as_ptr(), b.len() as u32, 0);
            assert!(n > 0, "「{q}」の展開が空");
            let out = core::slice::from_raw_parts(az_out_ptr(), az_out_len() as usize);
            let mut forms = Vec::new();
            let mut p = 0usize;
            for _ in 0..n {
                let ratio = u32::from_le_bytes(out[p..p + 4].try_into().unwrap());
                let len = u32::from_le_bytes(out[p + 4..p + 8].try_into().unwrap()) as usize;
                p += 8;
                assert!(ratio <= 100, "旧仮名率が {ratio}");
                forms.push(String::from_utf8(out[p..p + len].to_vec()).expect("不正な UTF-8"));
                p += len;
            }
            assert_eq!(p, out.len(), "レコードの長さが合わない");
            assert!(
                forms.contains(&want.to_string()),
                "「{q}」→「{want}」が無い: {forms:?}"
            );
            assert!(!forms.contains(&q.to_string()), "元の語が混ざっている");
        }
        // 衝突しやすい規則は明示したときだけ出る
        let b = "おもう".as_bytes();
        az_variants(b.as_ptr(), b.len() as u32, 0);
        let plain = az_out_len();
        az_variants(b.as_ptr(), b.len() as u32, 1);
        assert!(az_out_len() > plain, "risky=1 で形が増えていない");
    }
}
