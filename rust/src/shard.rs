//! 配信形式 — 索引を小さなシャードに割り、必要な範囲だけ取りに行けるようにする。
//!
//! 全 4,998 作の索引は 199.4 MB あり、丸ごとブラウザへ配ることはできない。
//! かといって HTTP Range で細切れに読むと、位置復元(LF を逐次に辿る)で
//! 往復が数千回に達して成立しない。
//!
//! そこで**小さく割って、必要なシャードだけ丸ごと落とす**。
//! 逐次の読みがメモリ内に閉じるので往復は 1 シャードにつき 1 回で済む。
//!
//! ## ファイルの並び
//!
//! ```text
//! [ヘッダ 64B][作品表][メタ][rank 標本] │ [SA 標本][ビット列本体]
//!  ← ここまでが常駐領域(先頭からの 1 範囲で取れる) →   ← 随時 →
//! ```
//!
//! 常駐領域だけを全シャードぶん集めれば、**どのシャードに何件あるか**を
//! 数えられる。表示する段になってはじめて、そのシャードの残りを取りに行く。

use crate::fm::{FmIndex, SA_SAMPLE};

pub const MAGIC: u32 = 0x4b53_5a41; // "AZSK"
/// 配信形式の版。**並びを変えたら必ず上げる**。
/// 上げ忘れると旧形式のファイルが検査を通過して誤読される(実際に起きた)。
/// 忘れないよう、形式の指紋をゲートに置いてある(O-12)。
pub const VERSION: u32 = 2;
pub const HEADER: usize = 64;

/// シャードに入っている作品の並び
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doc {
    /// 青空文庫の作品 ID
    pub id: u32,
    /// シャード本文の先頭からのバイト位置
    pub offset: u32,
}

pub struct Shard {
    pub fm: FmIndex,
    pub docs: Vec<Doc>,
}

impl Shard {
    /// 本文と作品表からシャードを組む
    pub fn build(text: &[u8], docs: Vec<Doc>) -> Self {
        Shard {
            fm: FmIndex::build(text),
            docs,
        }
    }

    /// 位置 `pos` を含む作品の添字
    pub fn doc_of(&self, pos: usize) -> usize {
        match self.docs.binary_search_by_key(&(pos as u32), |d| d.offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let (mut meta, mut sb, mut sample, mut words) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        self.fm
            .write_parts(&mut meta, &mut sb, &mut sample, &mut words);

        let mut docs = Vec::with_capacity(self.docs.len() * 8);
        for d in &self.docs {
            docs.extend_from_slice(&d.id.to_le_bytes());
            docs.extend_from_slice(&d.offset.to_le_bytes());
        }

        let mut out = vec![0u8; HEADER];
        let put = |out: &mut Vec<u8>, part: &[u8]| -> (u64, u64) {
            let at = out.len() as u64;
            out.extend_from_slice(part);
            (at, part.len() as u64)
        };
        let (docs_at, docs_len) = put(&mut out, &docs);
        let (meta_at, meta_len) = put(&mut out, &meta);
        let (sb_at, sb_len) = put(&mut out, &sb);
        let resident = out.len() as u64; // ここまでが常駐領域
        let (sample_at, sample_len) = put(&mut out, &sample);
        let (words_at, words_len) = put(&mut out, &words);

        let mut h = Vec::with_capacity(HEADER);
        h.extend_from_slice(&MAGIC.to_le_bytes());
        h.extend_from_slice(&VERSION.to_le_bytes());
        h.extend_from_slice(&(SA_SAMPLE as u32).to_le_bytes());
        h.extend_from_slice(&(self.docs.len() as u32).to_le_bytes());
        h.extend_from_slice(&resident.to_le_bytes());
        for (at, len) in [
            (docs_at, docs_len),
            (meta_at, meta_len),
            (sb_at, sb_len),
            (sample_at, sample_len),
            (words_at, words_len),
        ] {
            h.extend_from_slice(&(at as u32).to_le_bytes());
            h.extend_from_slice(&(len as u32).to_le_bytes());
        }
        assert!(h.len() <= HEADER, "ヘッダが {HEADER} バイトに収まらない");
        out[..h.len()].copy_from_slice(&h);
        out
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, String> {
        if buf.len() < HEADER {
            return Err("短すぎる".into());
        }
        let u32at = |p: usize| u32::from_le_bytes(buf[p..p + 4].try_into().unwrap());
        if u32at(0) != MAGIC {
            return Err("magic が違う".into());
        }
        if u32at(4) != VERSION {
            return Err(format!("版が違う: {} (期待 {VERSION})", u32at(4)));
        }
        let interval = u32at(8) as usize;
        if interval != SA_SAMPLE {
            return Err(format!("SA 標本間隔が違う: {interval} (期待 {SA_SAMPLE})"));
        }
        let n_docs = u32at(12) as usize;
        let region = |i: usize| {
            let at = u32at(24 + i * 8) as usize;
            let len = u32at(28 + i * 8) as usize;
            &buf[at..at + len]
        };
        let docs_b = region(0);
        let mut docs = Vec::with_capacity(n_docs);
        for i in 0..n_docs {
            docs.push(Doc {
                id: u32::from_le_bytes(docs_b[i * 8..i * 8 + 4].try_into().unwrap()),
                offset: u32::from_le_bytes(docs_b[i * 8 + 4..i * 8 + 8].try_into().unwrap()),
            });
        }
        Ok(Shard {
            fm: FmIndex::read_parts(region(1), region(2), region(3), region(4)),
            docs,
        })
    }

    /// (常駐領域, 全体) のバイト数
    pub fn region_bytes(&self) -> (usize, usize) {
        let (sb, sample, words) = self.fm.region_bytes();
        let meta = HEADER + self.docs.len() * 8;
        // メタ本体の実寸はヘッダ経由でしか分からないので、書き出して測る
        let bytes = self.to_bytes();
        let resident = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
        let _ = (sb, sample, words, meta);
        (resident, bytes.len())
    }
}

/// 形式の指紋 — 固定入力を直列化したバイト列の FNV-1a ハッシュ。
///
/// 並びを変えるとこの値が変わる。ゲート(O-12)が落ちたら、`VERSION` を上げて
/// から指紋を更新すること。順序を逆にしてはならない。
pub fn format_fingerprint() -> u64 {
    let text: Vec<u8> = (0..512u32).map(|i| (i % 7 + 1) as u8).collect();
    let docs = vec![
        Doc { id: 42, offset: 0 },
        Doc {
            id: 43,
            offset: 256,
        },
    ];
    let bytes = Shard::build(&text, docs).to_bytes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in &bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// 本文の総量が `target` バイトを超えないように作品をシャードへ割り振る。
///
/// 作品はまたがせない — 1 作が 1 シャードに収まる。したがって長い作品は
/// 単独で 1 シャードになる(実測の最長は 1,966,432 バイト)。
pub fn plan_shards(sizes: &[usize], target: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (mut start, mut acc) = (0usize, 0usize);
    for (i, &s) in sizes.iter().enumerate() {
        if acc > 0 && acc + s > target {
            out.push((start, i));
            start = i;
            acc = 0;
        }
        acc += s;
    }
    if start < sizes.len() {
        out.push((start, sizes.len()));
    }
    out
}

// ---------------------------------------------------------------- 部分読み

/// ファイル内の位置。JS はこの範囲を Range で取りに行く
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteRange {
    pub at: u32,
    pub len: u32,
}

/// 常駐領域だけを読んだシャード。ワード列は持たず、必要になった分だけ供給する。
///
/// 全 228 枚を丸ごと落とすと 232.9 MB になるが、計数に要るワードだけなら
/// 1 語あたり数十個で済む。位置復元と文脈取り出しは LF が数百段の逐次なので
/// この経路では扱わない — 表示する数枚だけ丸ごと落とす。
pub struct PartialShard {
    pub fm: FmIndex,
    pub docs: Vec<Doc>,
    /// ファイル内でワード列が始まる位置
    words_at: u32,
    /// 各ノードのワード列が、ワード領域の中で始まる位置(ワード数)
    node_offsets: Vec<u32>,
}

impl PartialShard {
    /// 常駐領域(ファイル先頭から `resident` バイト)だけから組む
    pub fn from_resident(buf: &[u8]) -> Result<Self, String> {
        if buf.len() < HEADER {
            return Err("短すぎる".into());
        }
        let u32at = |p: usize| u32::from_le_bytes(buf[p..p + 4].try_into().unwrap());
        if u32at(0) != MAGIC {
            return Err("magic が違う".into());
        }
        if u32at(4) != VERSION {
            return Err(format!("版が違う: {} (期待 {VERSION})", u32at(4)));
        }
        let n_docs = u32at(12) as usize;
        let resident = u64::from_le_bytes(buf[16..24].try_into().unwrap()) as usize;
        if buf.len() < resident {
            return Err(format!("常駐領域が足りない: {} < {resident}", buf.len()));
        }
        let region = |i: usize| -> &[u8] {
            let at = u32at(24 + i * 8) as usize;
            let len = u32at(28 + i * 8) as usize;
            if at + len <= buf.len() {
                &buf[at..at + len]
            } else {
                &[]
            }
        };
        let docs_b = region(0);
        let mut docs = Vec::with_capacity(n_docs);
        for i in 0..n_docs {
            docs.push(Doc {
                id: u32::from_le_bytes(docs_b[i * 8..i * 8 + 4].try_into().unwrap()),
                offset: u32::from_le_bytes(docs_b[i * 8 + 4..i * 8 + 8].try_into().unwrap()),
            });
        }
        let fm = FmIndex::read_parts_absent(region(1), region(2), region(3));
        let node_offsets = fm.node_word_offsets();
        Ok(PartialShard {
            fm,
            docs,
            words_at: u32at(24 + 4 * 8),
            node_offsets,
        })
    }

    /// 常駐領域の大きさ(ヘッダだけ読めば分かる)
    pub fn resident_len(header: &[u8]) -> Option<usize> {
        if header.len() < HEADER || u32::from_le_bytes(header[0..4].try_into().ok()?) != MAGIC {
            return None;
        }
        Some(u64::from_le_bytes(header[16..24].try_into().ok()?) as usize)
    }

    /// 語を数える。足りなければ `Err(要るバイト範囲)` を返す
    pub fn try_count(&self, pattern: &[u8]) -> Result<usize, Vec<ByteRange>> {
        let mut missing = Vec::new();
        match self.fm.try_count(pattern, &mut missing) {
            Some(n) => Ok(n),
            None => {
                missing.sort_unstable();
                missing.dedup();
                let mut out: Vec<ByteRange> = missing
                    .into_iter()
                    .map(|(node, w)| ByteRange {
                        at: self.words_at + (self.node_offsets[node as usize] + w) * 8,
                        len: 8,
                    })
                    .collect();
                out.sort_by_key(|r| r.at);
                // 隣り合う範囲はまとめて 1 回の読みにする
                let mut merged: Vec<ByteRange> = Vec::with_capacity(out.len());
                for r in out {
                    match merged.last_mut() {
                        Some(p) if p.at + p.len >= r.at => {
                            p.len = (r.at + r.len).saturating_sub(p.at);
                        }
                        _ => merged.push(r),
                    }
                }
                Err(merged)
            }
        }
    }

    /// ファイル位置 `at` から始まるバイト列を供給する
    pub fn supply(&mut self, at: u32, bytes: &[u8]) {
        if at < self.words_at {
            return;
        }
        let first = ((at - self.words_at) / 8) as usize;
        for (k, chunk) in bytes.chunks_exact(8).enumerate() {
            let w = first + k;
            // どのノードの何番目のワードか
            let node = match self.node_offsets.binary_search(&(w as u32)) {
                Ok(mut i) => {
                    // 同じ開始位置を持つノード(葉)が並ぶので、最後の一つを取る
                    while i + 1 < self.node_offsets.len() && self.node_offsets[i + 1] == w as u32 {
                        i += 1;
                    }
                    i
                }
                Err(i) => i.saturating_sub(1),
            };
            let idx = w - self.node_offsets[node] as usize;
            self.fm
                .supply(node, idx, u64::from_le_bytes(chunk.try_into().unwrap()));
        }
    }
}
