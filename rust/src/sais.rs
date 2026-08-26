//! 接尾辞配列の構築(SA-IS / Nong-Zhang-Chan)。線形時間。
//!
//! 素の接尾辞配列は 4 バイト整数 × n なので、正味 70 MB の本文に対して 280 MB になる。
//! 配信するのは BWT と疎な SA サンプルであって、この配列そのものではない。
//! ここはビルド時にだけ走る。
//!
//! 正しさは総当たり実装との照合で担保する(`tests/oracle.rs` の O-SA)。
//! 単独で「たぶん合っている」と判断してはならない。

const EMPTY: u32 = u32::MAX;

/// バイト列の接尾辞配列。末尾に番兵 0 を足して扱うため、`s` に 0 を含めてはならない
/// (UTF-8 の本文は 0 バイトを含まない — ゲート O-NUL で検査する)。
pub fn suffix_array(s: &[u8]) -> Vec<u32> {
    debug_assert!(!s.contains(&0), "本文に NUL バイトが含まれている");
    if s.is_empty() {
        return Vec::new();
    }
    let mut t: Vec<u32> = Vec::with_capacity(s.len() + 1);
    t.extend(s.iter().map(|&b| b as u32 + 1));
    t.push(0); // 番兵(最小かつ唯一)
    let sa = sais(&t, 258);
    // 番兵の分を落とす
    sa.into_iter().skip(1).collect()
}

/// SA-IS 本体。`s` は末尾に唯一最小の 0 を持ち、値は `0..k` に収まる。
fn sais(s: &[u32], k: usize) -> Vec<u32> {
    let n = s.len();
    if n == 1 {
        return vec![0];
    }
    if n == 2 {
        return if s[0] < s[1] { vec![0, 1] } else { vec![1, 0] };
    }

    // S 型 / L 型の分類。末尾の番兵は S 型
    let mut is_s = vec![false; n];
    is_s[n - 1] = true;
    for i in (0..n - 1).rev() {
        is_s[i] = match s[i].cmp(&s[i + 1]) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Equal => is_s[i + 1],
        };
    }
    let is_lms = |i: usize, is_s: &[bool]| i > 0 && is_s[i] && !is_s[i - 1];

    let mut counts = vec![0u32; k];
    for &c in s {
        counts[c as usize] += 1;
    }

    let mut sa = vec![EMPTY; n];

    // 手順 1 — LMS をバケット末尾に置いて誘導ソート
    let mut tails = bucket_tails(&counts);
    for i in (1..n).rev() {
        if is_lms(i, &is_s) {
            let c = s[i] as usize;
            tails[c] -= 1;
            sa[tails[c] as usize] = i as u32;
        }
    }
    induce(s, &mut sa, &is_s, &counts);

    // 手順 2 — 並んだ LMS を前に詰め、LMS 部分文字列に名前を付ける
    let mut n1 = 0usize;
    for i in 0..n {
        let j = sa[i];
        if j != EMPTY && is_lms(j as usize, &is_s) {
            sa[n1] = j;
            n1 += 1;
        }
    }
    for slot in sa.iter_mut().take(n).skip(n1) {
        *slot = EMPTY;
    }

    let mut name = 0u32;
    let mut prev = usize::MAX;
    for i in 0..n1 {
        let pos = sa[i] as usize;
        if prev == usize::MAX || !lms_substring_eq(s, &is_s, prev, pos, n) {
            name += 1;
            prev = pos;
        }
        sa[n1 + pos / 2] = name - 1;
    }
    let mut j = n;
    for i in (n1..n).rev() {
        if sa[i] != EMPTY {
            j -= 1;
            sa[j] = sa[i];
        }
    }

    // 縮約した列を再帰的に解く。名前がすべて相異なれば再帰は不要
    let s1: Vec<u32> = sa[n - n1..n].to_vec();
    let sa1 = if (name as usize) < n1 {
        sais(&s1, name as usize)
    } else {
        let mut r = vec![0u32; n1];
        for (i, &c) in s1.iter().enumerate() {
            r[c as usize] = i as u32;
        }
        r
    };

    // 手順 3 — 元の位置に戻し、もう一度誘導ソート
    let mut lms_pos = Vec::with_capacity(n1);
    for i in 1..n {
        if is_lms(i, &is_s) {
            lms_pos.push(i as u32);
        }
    }
    for slot in sa.iter_mut() {
        *slot = EMPTY;
    }
    let mut tails = bucket_tails(&counts);
    for i in (0..n1).rev() {
        let p = lms_pos[sa1[i] as usize];
        let c = s[p as usize] as usize;
        tails[c] -= 1;
        sa[tails[c] as usize] = p;
    }
    induce(s, &mut sa, &is_s, &counts);
    sa
}

fn bucket_heads(counts: &[u32]) -> Vec<u32> {
    let mut acc = 0u32;
    counts
        .iter()
        .map(|&c| {
            let h = acc;
            acc += c;
            h
        })
        .collect()
}

fn bucket_tails(counts: &[u32]) -> Vec<u32> {
    let mut acc = 0u32;
    counts
        .iter()
        .map(|&c| {
            acc += c;
            acc
        })
        .collect()
}

/// 誘導ソート — L 型を左から、S 型を右から埋める
fn induce(s: &[u32], sa: &mut [u32], is_s: &[bool], counts: &[u32]) {
    let n = s.len();
    let mut heads = bucket_heads(counts);
    for i in 0..n {
        let j = sa[i];
        if j != EMPTY && j > 0 {
            let p = j as usize - 1;
            if !is_s[p] {
                let c = s[p] as usize;
                sa[heads[c] as usize] = p as u32;
                heads[c] += 1;
            }
        }
    }
    let mut tails = bucket_tails(counts);
    for i in (0..n).rev() {
        let j = sa[i];
        if j != EMPTY && j > 0 {
            let p = j as usize - 1;
            if is_s[p] {
                let c = s[p] as usize;
                tails[c] -= 1;
                sa[tails[c] as usize] = p as u32;
            }
        }
    }
}

/// 2 つの LMS 部分文字列が等しいか
fn lms_substring_eq(s: &[u32], is_s: &[bool], a: usize, b: usize, n: usize) -> bool {
    if a == n - 1 || b == n - 1 {
        return a == b;
    }
    let is_lms = |i: usize| i > 0 && is_s[i] && !is_s[i - 1];
    let mut i = 0usize;
    loop {
        if a + i >= n || b + i >= n {
            return false;
        }
        let a_lms = is_lms(a + i);
        let b_lms = is_lms(b + i);
        if i > 0 && a_lms && b_lms {
            return true;
        }
        if a_lms != b_lms || s[a + i] != s[b + i] {
            return false;
        }
        i += 1;
    }
}

/// 総当たりによる参照実装。SA-IS の照合相手であって、出荷経路では使わない。
/// 計算量は O(n^2 log n) 相当なので小さな入力にしか使えない。
pub fn suffix_array_naive(s: &[u8]) -> Vec<u32> {
    let mut sa: Vec<u32> = (0..s.len() as u32).collect();
    sa.sort_by(|&a, &b| s[a as usize..].cmp(&s[b as usize..]));
    sa
}
