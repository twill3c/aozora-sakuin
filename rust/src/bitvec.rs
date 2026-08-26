//! rank を定数時間で引けるビット列。
//!
//! ウェーブレット木の各層がこれを持つ。配信サイズの大半を占めるのはここなので、
//! 標本の粒度がそのまま索引の大きさに効く。
//!
//! 構成: 64 ビットのワード列 + 512 ビットごとの累積(u32) + ワードごとの popcount。
//! 追加コストは 1 ビットあたり 512 分の 32 = 6.25%。

pub struct BitVec {
    words: Vec<u64>,
    /// 512 ビット(= 8 ワード)ごとの、そこまでの 1 の総数
    superblocks: Vec<u32>,
    len: usize,
    /// ワードが手元にあるか。空なら全て在る(丸ごと読み込んだ場合)。
    /// Range で必要な分だけ読む場合はここが疎になり、無いところに触ると `None` を返す
    present: Vec<bool>,
}

const WORD: usize = 64;
const SUPER: usize = 8; // ワード数 = 512 ビット

impl BitVec {
    pub fn from_bits(bits: &[bool]) -> Self {
        let len = bits.len();
        let nwords = (len + WORD - 1) / WORD;
        let mut words = vec![0u64; nwords];
        for (i, &b) in bits.iter().enumerate() {
            if b {
                words[i / WORD] |= 1u64 << (i % WORD);
            }
        }
        Self::from_words(words, len)
    }

    fn from_words(words: Vec<u64>, len: usize) -> Self {
        let mut superblocks = Vec::with_capacity((words.len() + SUPER - 1) / SUPER + 1);
        let mut acc = 0u32;
        for (i, w) in words.iter().enumerate() {
            if i % SUPER == 0 {
                superblocks.push(acc);
            }
            acc += w.count_ones();
        }
        superblocks.push(acc);
        BitVec {
            words,
            superblocks,
            len,
            present: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 位置 i のビット
    pub fn get(&self, i: usize) -> bool {
        debug_assert!(i < self.len);
        self.words[i / WORD] >> (i % WORD) & 1 == 1
    }

    /// 先頭から i 個(i は含まない)のうち 1 の個数
    pub fn rank1(&self, i: usize) -> usize {
        debug_assert!(i <= self.len);
        let word = i / WORD;
        let sb = word / SUPER;
        let mut acc = self.superblocks[sb] as usize;
        for w in &self.words[sb * SUPER..word] {
            acc += w.count_ones() as usize;
        }
        let rem = i % WORD;
        if rem > 0 {
            acc += (self.words[word] & ((1u64 << rem) - 1)).count_ones() as usize;
        }
        acc
    }

    pub fn rank0(&self, i: usize) -> usize {
        i - self.rank1(i)
    }

    /// 配信サイズ(バイト)
    pub fn size_bytes(&self) -> usize {
        self.words.len() * 8 + self.superblocks.len() * 4
    }

    /// 総当たりによる参照実装(照合用)
    pub fn rank1_naive(bits: &[bool], i: usize) -> usize {
        bits[..i].iter().filter(|&&b| b).count()
    }

    // --- 直列化のための素の参照。配信形式(shard.rs)が使う ---

    pub fn words(&self) -> &[u64] {
        &self.words
    }

    pub fn superblocks(&self) -> &[u32] {
        &self.superblocks
    }

    /// 直列化した部品から組み直す。`superblocks` は再計算せず受け取った値を信じる
    pub fn from_parts(words: Vec<u64>, superblocks: Vec<u32>, len: usize) -> Self {
        BitVec {
            words,
            superblocks,
            len,
            present: Vec::new(),
        }
    }

    /// 512 ビットごとの標本の個数
    pub fn superblock_bytes(&self) -> usize {
        self.superblocks.len() * 4
    }

    pub fn word_bytes(&self) -> usize {
        self.words.len() * 8
    }

    // --- 部分読み。Range で必要なワードだけ取りに行くための入口 ---

    /// ワードを持たない状態で組む。rank 標本だけが手元にある場合。
    pub fn from_parts_absent(n_words: usize, superblocks: Vec<u32>, len: usize) -> Self {
        BitVec {
            words: vec![0; n_words],
            superblocks,
            len,
            present: vec![false; n_words],
        }
    }

    /// ワードを 1 個供給する
    pub fn supply(&mut self, index: usize, word: u64) {
        if index < self.words.len() {
            self.words[index] = word;
            if !self.present.is_empty() {
                self.present[index] = true;
            }
        }
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    /// `present` が空なら「全部ある」(丸ごと読み込んだ場合)
    fn have(&self, i: usize) -> bool {
        self.present.is_empty() || self.present.get(i).copied().unwrap_or(false)
    }

    /// 手元に無いワードに触れたら `None` を返し、その番号を `missing` に積む。
    ///
    /// 丸ごと読み込んだ場合は常に `Some` で、`rank1` と同じ値になる。
    /// **この一致が O-03 の照合対象**。
    pub fn try_rank1(&self, i: usize, missing: &mut Vec<u32>) -> Option<usize> {
        debug_assert!(i <= self.len);
        let word = i / WORD;
        let sb = word / SUPER;
        let mut acc = self.superblocks[sb] as usize;
        let mut ok = true;
        for w in sb * SUPER..word {
            if self.have(w) {
                acc += self.words[w].count_ones() as usize;
            } else {
                missing.push(w as u32);
                ok = false;
            }
        }
        let rem = i % WORD;
        if rem > 0 {
            if self.have(word) {
                acc += (self.words[word] & ((1u64 << rem) - 1)).count_ones() as usize;
            } else {
                missing.push(word as u32);
                ok = false;
            }
        }
        if ok {
            Some(acc)
        } else {
            None
        }
    }

    pub fn try_rank0(&self, i: usize, missing: &mut Vec<u32>) -> Option<usize> {
        self.try_rank1(i, missing).map(|r| i - r)
    }
}
