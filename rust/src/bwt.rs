//! Burrows-Wheeler 変換と、その逆変換。
//!
//! BWT は接尾辞配列から O(n) で得られる。逆変換は LF 写像を辿るだけで、
//! 接尾辞配列を持たずに原文を復元できる — これが「索引だけを配信して
//! 原文を配らない」ことを可能にしている。
//!
//! 逆変換は**正しさの根拠にはならない**(HC-027)。自己無矛盾を示すだけで、
//! 原文の解釈が正しいことは示さない。原文との直接照合を別に置く。

use crate::sais::suffix_array;

/// 番兵の位置を表す。BWT 列そのものには番兵を含めず、その位置を別に持つ。
pub struct Bwt {
    /// 長さ n のバイト列。`primary` 番目に「原文の末尾の次」が来る
    pub last: Vec<u8>,
    /// 番兵(原文全体の接尾辞)が BWT 上で占める行番号
    pub primary: usize,
}

/// 原文から BWT を作る。`s` に 0 バイトを含めてはならない。
pub fn transform(s: &[u8]) -> Bwt {
    let sa = suffix_array(s);
    transform_with_sa(s, &sa)
}

/// 接尾辞配列を既に持っている場合はこちらを使う。
/// 接尾辞配列の構築は索引作りで最も重い工程なので、二度作らない。
pub fn transform_with_sa(s: &[u8], sa: &[u32]) -> Bwt {
    let n = s.len();
    let mut last = Vec::with_capacity(n);
    let mut primary = 0usize;
    // 番兵行(接尾辞 = 原文全体)は SA の先頭に来る。その行の直前文字は原文の末尾
    last.push(if n == 0 { 0 } else { s[n - 1] });
    for (row, &i) in sa.iter().enumerate() {
        if i == 0 {
            primary = row + 1; // +1 は番兵行の分
            last.push(0); // この行の直前は番兵
        } else {
            last.push(s[i as usize - 1]);
        }
    }
    last.truncate(n + 1);
    Bwt { last, primary }
}

/// BWT から原文を復元する(LF 写像)。
pub fn inverse(b: &Bwt) -> Vec<u8> {
    let m = b.last.len();
    if m <= 1 {
        return Vec::new();
    }
    // 各文字の出現数 → 先頭列における開始位置
    let mut counts = [0u32; 256];
    for (row, &c) in b.last.iter().enumerate() {
        if row != b.primary {
            counts[c as usize] += 1;
        }
    }
    let mut starts = [0u32; 256];
    let mut acc = 1u32; // 行 0 は番兵
    for c in 0..256 {
        starts[c] = acc;
        acc += counts[c];
    }
    // LF[row] = starts[c] + (row までに現れた c の個数)
    let mut seen = [0u32; 256];
    let mut lf = vec![0u32; m];
    for (row, (slot, &ch)) in lf.iter_mut().zip(b.last.iter()).enumerate() {
        if row == b.primary {
            *slot = 0;
            continue;
        }
        let c = ch as usize;
        *slot = starts[c] + seen[c];
        seen[c] += 1;
    }
    // 行 0 は接尾辞 = 番兵。その直前文字が原文の末尾 T[n-1] なので、
    // 「読んでから LF を辿る」順序でなければ 1 文字ずれる
    let mut out = vec![0u8; m - 1];
    let mut row = 0usize;
    for i in (0..m - 1).rev() {
        out[i] = b.last[row];
        row = lf[row] as usize;
    }
    out
}

/// 総当たりによる参照実装。全巡回シフトを並べて最終列を取る。
/// 計算量が O(n^2 log n) なので小さな入力にしか使えない。
pub fn transform_naive(s: &[u8]) -> Vec<u8> {
    let n = s.len();
    let mut rot: Vec<Vec<u8>> = Vec::with_capacity(n + 1);
    // 番兵 0 を末尾に足した長さ n+1 の列の巡回シフト
    let mut t = s.to_vec();
    t.push(0);
    for i in 0..=n {
        let mut r = t[i..].to_vec();
        r.extend_from_slice(&t[..i]);
        rot.push(r);
    }
    rot.sort();
    rot.iter().map(|r| r[n]).collect()
}
