//! ブラウザとの境界(SPEC F-04 / N-01 / N-02)。
//!
//! `extern "C"` の整数関数だけを出す。wasm-bindgen は使わない。
//!
//! ## JS は索引の構造を知らない
//!
//! JS が触れるのは「シャードのバイト列を渡す」「語を渡す」「結果のレコード列を読む」
//! の 3 つだけ。BWT もウェーブレット木も LF 写像も、境界の向こう側にある。
//! 位置の計算も文脈の取り出しも全て Rust 側で行う。
//!
//! ## 結果のレコード
//!
//! `az_kwic` は出力バッファに次の並びを詰める。JS はこの並びだけを知っていればよい。
//!
//! ```text
//! 1 件につき:
//!   u32 作品ID
//!   u32 作品内のバイト位置
//!   u32 前文脈の長さ
//!   u32 一致部の長さ
//!   u32 後文脈の長さ
//!   前文脈 + 一致部 + 後文脈 (UTF-8)
//! ```

use crate::shard::Shard;
use crate::variants::expand;

/// 読み込んだシャード。wasm は単一スレッドなので `static mut` で足りる
static mut SHARDS: Vec<Option<Shard>> = Vec::new();
/// 結果の詰め先
static mut OUT: Vec<u8> = Vec::new();

/// # Safety
/// 返した領域は `az_free` で同じ長さを渡して解放すること。
#[no_mangle]
pub extern "C" fn az_alloc(len: u32) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len as usize);
    let p = v.as_mut_ptr();
    core::mem::forget(v);
    p
}

/// # Safety
/// `ptr` は `az_alloc(len)` が返したものでなければならない。
#[no_mangle]
pub unsafe extern "C" fn az_free(ptr: *mut u8, len: u32) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, 0, len as usize));
    }
}

/// シャードのバイト列を読み込み、取っ手(0 以上)を返す。読めなければ -1。
///
/// # Safety
/// `ptr` から `len` バイトが有効であること。
#[no_mangle]
pub unsafe extern "C" fn az_shard_load(ptr: *const u8, len: u32) -> i32 {
    if ptr.is_null() {
        return -1;
    }
    let buf = core::slice::from_raw_parts(ptr, len as usize);
    match Shard::from_bytes(buf) {
        Ok(s) => {
            let slot = SHARDS.iter().position(|x| x.is_none());
            match slot {
                Some(i) => {
                    SHARDS[i] = Some(s);
                    i as i32
                }
                None => {
                    SHARDS.push(Some(s));
                    (SHARDS.len() - 1) as i32
                }
            }
        }
        Err(_) => -1,
    }
}

/// # Safety
/// `h` は `az_shard_load` が返した取っ手であること。
#[no_mangle]
pub unsafe extern "C" fn az_shard_drop(h: i32) {
    if let Some(slot) = shard_slot(h) {
        SHARDS[slot] = None;
    }
}

unsafe fn shard_slot(h: i32) -> Option<usize> {
    let i = usize::try_from(h).ok()?;
    if i < SHARDS.len() && SHARDS[i].is_some() {
        Some(i)
    } else {
        None
    }
}

unsafe fn shard(h: i32) -> Option<&'static Shard> {
    shard_slot(h).and_then(|i| SHARDS[i].as_ref())
}

/// このシャードに入っている作品数。取っ手が無効なら -1
///
/// # Safety
/// `h` は有効な取っ手であること。
#[no_mangle]
pub unsafe extern "C" fn az_shard_docs(h: i32) -> i32 {
    match shard(h) {
        Some(s) => s.docs.len() as i32,
        None => -1,
    }
}

/// 語の出現回数。位置は求めないので、ヒット数によらず一定の速さで返る。
/// 取っ手が無効なら -1。
///
/// # Safety
/// `pat` から `pat_len` バイトが有効であること。
#[no_mangle]
pub unsafe extern "C" fn az_count(h: i32, pat: *const u8, pat_len: u32) -> i32 {
    let (Some(s), false) = (shard(h), pat.is_null()) else {
        return -1;
    };
    let p = core::slice::from_raw_parts(pat, pat_len as usize);
    s.fm.count(p) as i32
}

/// 前後文脈つきの用例を最大 `max` 件、出力バッファへ詰める。詰めた件数を返す。
/// 取っ手が無効なら -1。
///
/// # Safety
/// `pat` から `pat_len` バイトが有効であること。
#[no_mangle]
pub unsafe extern "C" fn az_kwic(
    h: i32,
    pat: *const u8,
    pat_len: u32,
    width: u32,
    max: u32,
) -> i32 {
    let (Some(s), false) = (shard(h), pat.is_null()) else {
        return -1;
    };
    let p = core::slice::from_raw_parts(pat, pat_len as usize);
    OUT.clear();
    if p.is_empty() || max == 0 {
        return 0;
    }

    let (lo, hi) = s.fm.range(p);
    let mut positions: Vec<usize> = s.fm.locate(p);
    let _ = (lo, hi);
    positions.truncate(max as usize);

    let mut n = 0i32;
    for pos in positions {
        let d = s.doc_of(pos);
        let doc_lo = s.docs[d].offset as usize;
        let doc_hi = s
            .docs
            .get(d + 1)
            .map(|x| x.offset as usize)
            .unwrap_or(s.fm.len());
        let (before, hit, after) = s.fm.kwic(pos, p.len(), width as usize, (doc_lo, doc_hi));
        OUT.extend_from_slice(&s.docs[d].id.to_le_bytes());
        OUT.extend_from_slice(&((pos - doc_lo) as u32).to_le_bytes());
        OUT.extend_from_slice(&(before.len() as u32).to_le_bytes());
        OUT.extend_from_slice(&(hit.len() as u32).to_le_bytes());
        OUT.extend_from_slice(&(after.len() as u32).to_le_bytes());
        OUT.extend_from_slice(&before);
        OUT.extend_from_slice(&hit);
        OUT.extend_from_slice(&after);
        n += 1;
    }
    n
}

/// 直前の `az_kwic` が詰めたバッファの先頭
#[no_mangle]
pub extern "C" fn az_out_ptr() -> *const u8 {
    unsafe { OUT.as_ptr() }
}

/// 直前の `az_kwic` が詰めたバッファの長さ
#[no_mangle]
pub extern "C" fn az_out_len() -> u32 {
    unsafe { OUT.len() as u32 }
}

/// 語の異体形を出力バッファに詰める。詰めた形の数を返す。
///
/// `risky` が 0 でなければ、較正で衝突が多いと分かった規則も含める。
/// 展開の規則も較正値も Rust 側にあり、JS は形の並びを受け取るだけ。
///
/// レコードの並び: `u32 旧仮名率(百分率) / u32 長さ / バイト列(UTF-8)`
///
/// # Safety
/// `pat` から `pat_len` バイトが有効な UTF-8 であること。
#[no_mangle]
pub unsafe extern "C" fn az_variants(pat: *const u8, pat_len: u32, risky: u32) -> i32 {
    OUT.clear();
    if pat.is_null() {
        return -1;
    }
    let bytes = core::slice::from_raw_parts(pat, pat_len as usize);
    let Ok(q) = core::str::from_utf8(bytes) else {
        return -1;
    };
    let forms = expand(q, risky != 0);
    for f in &forms {
        // その形を生んだ規則の較正値。複数当たる場合は最も低いものを取る
        // (最も衝突しやすい規則の信頼度で見せる)
        let ratio = crate::variants::RULES
            .iter()
            .filter(|r| f.contains(r.to) && !q.contains(r.to))
            .map(|r| r.old_ratio)
            .min()
            .unwrap_or(99);
        OUT.extend_from_slice(&(ratio as u32).to_le_bytes());
        OUT.extend_from_slice(&(f.len() as u32).to_le_bytes());
        OUT.extend_from_slice(f.as_bytes());
    }
    forms.len() as i32
}

/// 配信形式の版。JS 側が取得したファイルと照合できるように出しておく
#[no_mangle]
pub extern "C" fn az_format_version() -> u32 {
    crate::shard::VERSION
}
