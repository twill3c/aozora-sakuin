//! FM-index — BWT とウェーブレット木で「語がどこに何個あるか」を引く。
//!
//! 探索は後方探索(backward search)。パターンの末尾から 1 文字ずつ、BWT 上の
//! 行区間 `[lo, hi)` を狭めていく。区間の幅がそのまま出現回数になるので、
//! **件数は位置を 1 つも求めずに分かる**。位置が要るのは表示する分だけ。
//!
//! 位置の復元には接尾辞配列の疎な標本を使う。標本間隔 `SA_SAMPLE` を大きくすると
//! 索引は小さくなり、位置の復元が遅くなる。ここが配信サイズと応答時間の交換点。
//!
//! ## UTF-8 とバイト単位の探索
//!
//! 索引はバイト列に対して張る。UTF-8 は自己同期的で、正しい UTF-8 列は
//! 別の文字の途中から始まる位置には決して一致しない。したがってバイト単位で
//! 探しても、文字境界をまたぐ偽の一致は生じない。

use crate::bwt;
use crate::sais::suffix_array;
use crate::wavelet::WaveletTree;

/// 接尾辞配列の標本間隔。索引サイズと位置復元の速さの交換点
pub const SA_SAMPLE: usize = 64;

/// 位置 → 行の標本間隔。文脈の取り出しはここで決まる歩数だけ LF を辿る
pub const ISA_SAMPLE: usize = 64;

pub struct FmIndex {
    wt: WaveletTree,
    /// C[c] = BWT 全体で c より小さい文字の個数(番兵を含む)
    c: [u32; 257],
    /// 番兵が BWT 上で占める行
    primary: usize,
    /// **行番号**が SA_SAMPLE の倍数である行の、接尾辞位置。
    ///
    /// 位置側で標本を取ると「どの行が標本か」を示すビット列(n ビット + rank 標本 =
    /// 原文の 13.3%)が要る。行側で取れば添字計算だけで済み、その 13.3% が丸ごと消える。
    /// 代償として LF を辿る歩数に上界が無くなるので、実測で分布を押さえる(O-08)。
    sa_sample: Vec<u32>,
    /// 位置 → 行(接尾辞配列の逆写像 ISA)の標本。位置が ISA_SAMPLE の倍数の点だけ持つ。
    ///
    /// LF は行を辿ると本文を**後ろから前へ**読む。前方の文脈を取り出すには
    /// 「読み始めたい位置より後ろにある既知の行」が要る。それがこの標本。
    /// SA 標本(行→位置)とは向きが逆で、両方が要る。
    isa_sample: Vec<u32>,
    n: usize,
    /// 木のメタ部分(直列化したバイト列の残り)。部分読みで木を組み直すのに要る
    meta_tail: Vec<u8>,
}

impl FmIndex {
    pub fn build(text: &[u8]) -> Self {
        assert!(!text.contains(&0), "本文に NUL バイトが含まれている");
        let n = text.len();
        let sa = suffix_array(text);
        let b = bwt::transform_with_sa(text, &sa);

        let mut c = [0u32; 257];
        for (row, &ch) in b.last.iter().enumerate() {
            if row == b.primary {
                c[1] += 1; // 番兵は最小記号として 1 個数える
            } else {
                c[ch as usize + 1] += 1;
            }
        }
        for i in 1..257 {
            c[i] += c[i - 1];
        }

        // 標本: 行番号が SA_SAMPLE の倍数の行について、その接尾辞位置を持つ。
        // 行 0 は番兵(位置 n)、行 r+1 は sa[r]
        let rows = n + 1;
        let mut sample = Vec::with_capacity((rows + SA_SAMPLE - 1) / SA_SAMPLE);
        let mut row = 0usize;
        while row < rows {
            sample.push(if row == 0 { n as u32 } else { sa[row - 1] });
            row += SA_SAMPLE;
        }

        // ISA 標本: 位置が ISA_SAMPLE の倍数の点について、その位置の行番号。
        // 行 0 は位置 n、行 r+1 は位置 sa[r]
        let mut isa = vec![0u32; (rows + ISA_SAMPLE - 1) / ISA_SAMPLE];
        if n % ISA_SAMPLE == 0 {
            isa[n / ISA_SAMPLE] = 0;
        }
        for (r, &pos) in sa.iter().enumerate() {
            if pos as usize % ISA_SAMPLE == 0 {
                isa[pos as usize / ISA_SAMPLE] = (r + 1) as u32;
            }
        }

        FmIndex {
            wt: WaveletTree::new(&b.last),
            c,
            primary: b.primary,
            sa_sample: sample,
            isa_sample: isa,
            n,
            meta_tail: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// BWT 行 `row` の直前文字を辿る(LF 写像)
    fn lf(&self, row: usize) -> usize {
        if row == self.primary {
            return 0;
        }
        let ch = self.wt.access(row);
        // 番兵行より前にある同じ文字だけを数えたいが、番兵は最小記号なので
        // wt 上の値 0 と衝突しない(本文に 0 バイトは無い)
        self.c[ch as usize] as usize + self.wt.rank(ch, row)
    }

    /// パターンに一致する BWT 行区間 `[lo, hi)` を返す。幅が出現回数
    pub fn range(&self, pattern: &[u8]) -> (usize, usize) {
        if pattern.is_empty() {
            return (0, self.n + 1);
        }
        let mut lo = 0usize;
        let mut hi = self.n + 1;
        for &ch in pattern.iter().rev() {
            let base = self.c[ch as usize] as usize;
            lo = base + self.wt.rank(ch, lo);
            hi = base + self.wt.rank(ch, hi);
            if lo >= hi {
                return (0, 0);
            }
        }
        (lo, hi)
    }

    /// 出現回数。位置は求めない
    pub fn count(&self, pattern: &[u8]) -> usize {
        if pattern.is_empty() {
            return 0; // 参照実装(find_all_naive)と意味を揃える
        }
        let (lo, hi) = self.range(pattern);
        hi - lo
    }

    /// 行 `row` に対応する原文中の位置と、そこまでに辿った歩数
    fn locate_row_steps(&self, mut row: usize) -> (usize, usize) {
        let mut steps = 0usize;
        loop {
            if row % SA_SAMPLE == 0 {
                let pos = (self.sa_sample[row / SA_SAMPLE] as usize + steps) % (self.n + 1);
                return (pos, steps);
            }
            row = self.lf(row);
            steps += 1;
            debug_assert!(steps <= self.n + 1, "LF が閉路に入った");
        }
    }

    fn locate_row(&self, row: usize) -> usize {
        self.locate_row_steps(row).0
    }

    /// 全行について、標本に到達するまでの歩数の最大値。
    /// 行標本には理論上の上界が無いため、実測で押さえる(O-08)。
    pub fn max_locate_steps(&self) -> usize {
        (0..=self.n)
            .map(|r| self.locate_row_steps(r).1)
            .max()
            .unwrap_or(0)
    }

    /// 出現位置(原文の先頭からのバイト位置)。昇順に整列して返す
    pub fn locate(&self, pattern: &[u8]) -> Vec<usize> {
        if pattern.is_empty() {
            return Vec::new();
        }
        let (lo, hi) = self.range(pattern);
        let mut out: Vec<usize> = (lo..hi).map(|r| self.locate_row(r)).collect();
        out.sort_unstable();
        out
    }

    /// 位置 `start` から `len` バイトの本文を、**索引だけから**復元する。
    ///
    /// LF は本文を後ろから前へ読むので、まず「取り出したい範囲より後ろにある
    /// 既知の行」へ ISA 標本で跳び、そこから前へ辿って埋める。
    /// 辿る歩数は (len + ISA_SAMPLE) を超えない。
    ///
    /// これがあるので**原文を配信しなくてよい**。索引が本文を兼ねている。
    pub fn extract(&self, start: usize, len: usize) -> Vec<u8> {
        if len == 0 || start >= self.n {
            return Vec::new();
        }
        let end = (start + len).min(self.n);

        // 読み始める行 — end 以上で最小の標本点。無ければ末尾(行 0 = 位置 n)
        let cand = (end + ISA_SAMPLE - 1) / ISA_SAMPLE * ISA_SAMPLE;
        let (p, mut row) = if cand <= self.n {
            (cand, self.isa_sample[cand / ISA_SAMPLE] as usize)
        } else {
            (self.n, 0usize)
        };

        let mut out = vec![0u8; p - start];
        for k in (0..p - start).rev() {
            out[k] = self.wt.access(row);
            row = self.lf(row);
        }
        out.truncate(end - start);
        out
    }

    /// 文脈つきで取り出す。`(前文脈, 一致部, 後文脈)` をバイト列で返す。
    ///
    /// 範囲は `bounds`(その作品の先頭と末尾)で切り、隣の作品へはみ出さない。
    /// UTF-8 の途中で切れないよう、両端を文字境界まで詰める。
    pub fn kwic(
        &self,
        pos: usize,
        hit_len: usize,
        width: usize,
        bounds: (usize, usize),
    ) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let (doc_lo, doc_hi) = bounds;
        let lo = pos.saturating_sub(width).max(doc_lo);
        let hi = (pos + hit_len + width).min(doc_hi).min(self.n);
        let buf = self.extract(lo, hi - lo);
        let at = pos - lo;
        // 前側は継続バイトを読み飛ばせば先頭バイトに着く。
        // 後側は「継続バイトかどうか」だけでは足りない — 多バイト文字の先頭バイトだけが
        // 末尾に残る場合を見逃す。標準の検証器に有効な前置の長さを聞く
        let mut a = 0usize;
        while a < at && !is_utf8_start(buf[a]) {
            a += 1;
        }
        let tail = at + hit_len;
        let b = tail
            + match core::str::from_utf8(&buf[tail..]) {
                Ok(_) => buf.len() - tail,
                Err(e) => e.valid_up_to(),
            };
        (
            buf[a..at].to_vec(),
            buf[at..at + hit_len].to_vec(),
            buf[at + hit_len..b].to_vec(),
        )
    }

    pub fn size_bytes(&self) -> usize {
        self.wt.size_bytes() + 257 * 4 + (self.sa_sample.len() + self.isa_sample.len()) * 4
    }

    /// 本文 1 バイトあたりの平均符号長(ビット)
    pub fn mean_code_len(&self, text: &[u8]) -> f64 {
        self.wt.mean_code_len(text)
    }
}

/// UTF-8 の先頭バイトか(継続バイト 0b10xxxxxx でない)
fn is_utf8_start(b: u8) -> bool {
    b & 0xC0 != 0x80
}

/// 総当たりによる参照実装 — 重なりを許した全出現位置。
/// これが O-01 の照合相手であり、正しさの根拠である。
pub fn find_all_naive(text: &[u8], pattern: &[u8]) -> Vec<usize> {
    if pattern.is_empty() || pattern.len() > text.len() {
        return Vec::new();
    }
    (0..=text.len() - pattern.len())
        .filter(|&i| &text[i..i + pattern.len()] == pattern)
        .collect()
}

// ---------------------------------------------------------------- 直列化

impl FmIndex {
    /// 配信形式の 4 領域へ書き出す。
    ///
    /// `meta` と `sb`(rank 標本)は**常駐**させる領域、`sample`(SA 標本)と
    /// `words`(ビット列本体)は必要になってから取りに行く領域。
    /// 件数を数えるだけなら meta + sb + words の一部で足り、位置を出す段になって
    /// はじめて sample が要る。
    pub fn write_parts(
        &self,
        meta: &mut Vec<u8>,
        sb: &mut Vec<u8>,
        sample: &mut Vec<u8>,
        words: &mut Vec<u8>,
    ) {
        meta.extend_from_slice(&(self.n as u64).to_le_bytes());
        meta.extend_from_slice(&(self.primary as u64).to_le_bytes());
        for v in self.c.iter() {
            meta.extend_from_slice(&v.to_le_bytes());
        }
        meta.extend_from_slice(&(self.sa_sample.len() as u32).to_le_bytes());
        meta.extend_from_slice(&(self.isa_sample.len() as u32).to_le_bytes());
        for &v in &self.sa_sample {
            sample.extend_from_slice(&v.to_le_bytes());
        }
        for &v in &self.isa_sample {
            sample.extend_from_slice(&v.to_le_bytes());
        }
        self.wt.write_parts(meta, sb, words);
    }

    pub fn read_parts(meta: &[u8], sb: &[u8], sample: &[u8], words: &[u8]) -> Self {
        let n = u64::from_le_bytes(meta[0..8].try_into().unwrap()) as usize;
        let primary = u64::from_le_bytes(meta[8..16].try_into().unwrap()) as usize;
        let mut c = [0u32; 257];
        let mut p = 16usize;
        for slot in c.iter_mut() {
            *slot = u32::from_le_bytes(meta[p..p + 4].try_into().unwrap());
            p += 4;
        }
        let n_sample = u32::from_le_bytes(meta[p..p + 4].try_into().unwrap()) as usize;
        p += 4;
        let n_isa = u32::from_le_bytes(meta[p..p + 4].try_into().unwrap()) as usize;
        p += 4;
        // 標本領域は常駐領域の外にある。計数だけなら位置を求めないので不要で、
        // 部分読みではここが空のまま渡ってくる
        let have_sample = sample.len() >= (n_sample + n_isa) * 4;
        let at = |i: usize| u32::from_le_bytes(sample[i * 4..i * 4 + 4].try_into().unwrap());
        let (sa_sample, isa_sample) = if have_sample {
            (
                (0..n_sample).map(at).collect(),
                (n_sample..n_sample + n_isa).map(at).collect(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        FmIndex {
            wt: WaveletTree::read_parts(&meta[p..], sb, words),
            c,
            primary,
            sa_sample,
            isa_sample,
            n,
            meta_tail: meta[p..].to_vec(),
        }
    }

    /// (rank 標本, SA 標本, ビット列本体) のバイト数
    pub fn region_bytes(&self) -> (usize, usize, usize) {
        let (sb, words) = self.wt.region_bytes();
        (
            sb,
            (self.sa_sample.len() + self.isa_sample.len()) * 4,
            words,
        )
    }
}

// ---------------------------------------------------------------- 部分読み

/// ワードを持たない索引。rank 標本と木の形だけが手元にあり、必要なワードは
/// 後から供給する。**計数だけなら位置を求めないので、これで足りる**。
///
/// 丸ごと読み込んだ索引と同じ答えを返さなければならない(O-03)。
impl FmIndex {
    /// メタと rank 標本だけから組む。ワード列は空
    pub fn read_parts_absent(meta: &[u8], sb: &[u8], sample: &[u8]) -> Self {
        let mut fm = Self::read_parts(meta, sb, sample, &[]);
        fm.wt = WaveletTree::read_parts_absent(&fm.meta_tail, sb);
        fm
    }

    /// ワードを 1 個供給する
    pub fn supply(&mut self, node: usize, word_index: usize, word: u64) {
        self.wt.supply(node, word_index, word);
    }

    /// 各ノードのワード列が、直列化したワード領域の中で始まる位置(ワード数)
    pub fn node_word_offsets(&self) -> Vec<u32> {
        self.wt.node_word_offsets()
    }

    /// 後方探索。ワードが足りなければ `None` を返し、要るワードを `missing` に積む。
    ///
    /// 途中まで進んだ状態は保持しない — 供給してから最初からやり直す。
    /// 1 段進むごとに必要なワードは 2 個だけなので、やり直しの費用は無視できる
    /// (すでに供給したワードは手元にあるため、次は先へ進む)。
    pub fn try_count(&self, pattern: &[u8], missing: &mut Vec<(u32, u32)>) -> Option<usize> {
        if pattern.is_empty() {
            return Some(0);
        }
        // このシャードに現れないバイトが 1 つでもあれば 0 件。
        // C 表は常駐領域にあるので、**ワードを 1 つも読まずに** 答えが出る
        for &ch in pattern {
            if self.c[ch as usize + 1] == self.c[ch as usize] {
                return Some(0);
            }
        }

        let mut lo = 0usize;
        let mut hi = self.n + 1;
        for (step, &ch) in pattern.iter().rev().enumerate() {
            let base = self.c[ch as usize] as usize;
            if step == 0 {
                // 最初の 1 歩は rank(c,0)=0 と rank(c,行数)=その文字の総数 で、
                // どちらも C 表から出る。ここでもワードを読まない
                lo = base;
                hi = self.c[ch as usize + 1] as usize;
            } else {
                let a = if lo == 0 {
                    Some(0)
                } else {
                    self.wt.try_rank(ch, lo, missing)
                };
                let b = self.wt.try_rank(ch, hi, missing);
                match (a, b) {
                    (Some(x), Some(y)) => {
                        lo = base + x;
                        hi = base + y;
                    }
                    _ => return None,
                }
            }
            if lo >= hi {
                return Some(0);
            }
        }
        Some(hi - lo)
    }
}
