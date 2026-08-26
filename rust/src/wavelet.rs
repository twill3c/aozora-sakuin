//! バイト列に対するウェーブレット木。
//!
//! 木の形を**バイトの出現頻度に合わせる**(ハフマン形)。平衡形は 1 バイトを
//! 8 ビットのまま持つので圧縮が一切かからないが、日本語 UTF-8 のバイト分布は
//! 継続バイトに強く偏っており(実測: 0 次エントロピー 4.72 ビット/バイト、
//! 異なるバイト値 158 種)、頻度に合わせるだけで 0.59 倍になる。
//!
//! 大きさは n×H0 ビット + rank 標本 6.25%。
//!
//! 各ノードが自分のビット列を持つので、平衡形で必要だったノード境界表は要らない。

use crate::bitvec::BitVec;

enum Node {
    Leaf(u8),
    Inner { bv: BitVec, left: u32, right: u32 },
}

pub struct WaveletTree {
    nodes: Vec<Node>,
    /// 符号(根からの経路。上位ビットが根側)と、その長さ。長さ 0 は不出現
    codes: [(u64, u8); 256],
    len: usize,
}

const MAX_CODE: u8 = 63;

impl WaveletTree {
    pub fn new(text: &[u8]) -> Self {
        let mut counts = [0u64; 256];
        for &b in text {
            counts[b as usize] += 1;
        }
        let present: Vec<u8> = (0..256u16)
            .filter(|&c| counts[c as usize] > 0)
            .map(|c| c as u8)
            .collect();

        let mut nodes: Vec<Node> = Vec::new();
        let mut codes = [(0u64, 0u8); 256];

        match present.len() {
            0 => {
                nodes.push(Node::Leaf(0));
                return WaveletTree {
                    nodes,
                    codes,
                    len: 0,
                };
            }
            1 => {
                // 1 種類しかない場合も根を内部ノードにして経路を揃える。
                // ビット列は全て 0 になる
                let s = present[0];
                codes[s as usize] = (0, 1);
                nodes.push(Node::Inner {
                    bv: BitVec::from_bits(&vec![false; text.len()]),
                    left: 1,
                    right: 1,
                });
                nodes.push(Node::Leaf(s));
                return WaveletTree {
                    nodes,
                    codes,
                    len: text.len(),
                };
            }
            _ => {}
        }

        // --- ハフマン木を組む ---
        // (重み, 決定論のための連番, ノード添字)。同重みの並びを安定させるため
        // 連番を第 2 キーに置く — 索引はビルドのたびに同一でなければならない
        struct Item {
            w: u64,
            seq: u32,
            node: u32,
        }
        let mut heap: Vec<Item> = Vec::with_capacity(present.len() * 2);
        let mut seq = 0u32;
        for &s in &present {
            nodes.push(Node::Leaf(s));
            heap.push(Item {
                w: counts[s as usize],
                seq,
                node: (nodes.len() - 1) as u32,
            });
            seq += 1;
        }
        // 小さい順に取り出す。要素数は最大 256 なので単純な選択で十分
        while heap.len() > 1 {
            heap.sort_by(|a, b| b.w.cmp(&a.w).then(b.seq.cmp(&a.seq))); // 降順
            let a = heap.pop().unwrap();
            let b = heap.pop().unwrap();
            nodes.push(Node::Inner {
                bv: BitVec::from_bits(&[]),
                left: a.node,
                right: b.node,
            });
            heap.push(Item {
                w: a.w + b.w,
                seq,
                node: (nodes.len() - 1) as u32,
            });
            seq += 1;
        }
        let root = heap[0].node as usize;

        // 根が末尾に来ているので 0 番へ移す(rank/access が root=0 を前提にする)
        nodes.swap(0, root);
        let fix = |i: u32| -> u32 {
            if i as usize == root {
                0
            } else if i == 0 {
                root as u32
            } else {
                i
            }
        };
        for n in nodes.iter_mut() {
            if let Node::Inner { left, right, .. } = n {
                *left = fix(*left);
                *right = fix(*right);
            }
        }

        // --- 符号を振る ---
        let mut stack = vec![(0usize, 0u64, 0u8)];
        while let Some((idx, code, depth)) = stack.pop() {
            match &nodes[idx] {
                Node::Leaf(s) => {
                    codes[*s as usize] = (code, depth.max(1));
                }
                Node::Inner { left, right, .. } => {
                    assert!(depth < MAX_CODE, "符号長が {MAX_CODE} を超えた");
                    stack.push((*left as usize, code << 1, depth + 1));
                    stack.push((*right as usize, code << 1 | 1, depth + 1));
                }
            }
        }

        let mut wt = WaveletTree {
            nodes,
            codes,
            len: text.len(),
        };
        wt.fill_bits(0, text, 0);
        wt
    }

    /// ノード `idx` に属する部分列 `seq` からビット列を作り、子へ分けて再帰する
    fn fill_bits(&mut self, idx: usize, seq: &[u8], depth: u8) {
        let (left, right) = match &self.nodes[idx] {
            Node::Leaf(_) => return,
            Node::Inner { left, right, .. } => (*left as usize, *right as usize),
        };
        let mut bits = Vec::with_capacity(seq.len());
        let mut l = Vec::new();
        let mut r = Vec::new();
        for &b in seq {
            let (code, clen) = self.codes[b as usize];
            let bit = code >> (clen - 1 - depth) & 1 == 1;
            bits.push(bit);
            if bit {
                r.push(b);
            } else {
                l.push(b);
            }
        }
        if let Node::Inner { bv, .. } = &mut self.nodes[idx] {
            *bv = BitVec::from_bits(&bits);
        }
        drop(bits);
        self.fill_bits(left, &l, depth + 1);
        drop(l);
        self.fill_bits(right, &r, depth + 1);
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 先頭 i 個(i は含まない)のうち、文字 c の個数
    pub fn rank(&self, c: u8, i: usize) -> usize {
        debug_assert!(i <= self.len);
        let (code, clen) = self.codes[c as usize];
        if clen == 0 {
            return 0; // 本文に現れない文字
        }
        let mut node = 0usize;
        let mut idx = i;
        for d in 0..clen {
            let bit = code >> (clen - 1 - d) & 1 == 1;
            match &self.nodes[node] {
                Node::Leaf(_) => break,
                Node::Inner { bv, left, right } => {
                    idx = if bit { bv.rank1(idx) } else { bv.rank0(idx) };
                    node = if bit { *right } else { *left } as usize;
                }
            }
        }
        idx
    }

    /// 位置 i の文字
    pub fn access(&self, i: usize) -> u8 {
        debug_assert!(i < self.len);
        let mut node = 0usize;
        let mut idx = i;
        loop {
            match &self.nodes[node] {
                Node::Leaf(s) => return *s,
                Node::Inner { bv, left, right } => {
                    let bit = bv.get(idx);
                    idx = if bit { bv.rank1(idx) } else { bv.rank0(idx) };
                    node = if bit { *right } else { *left } as usize;
                }
            }
        }
    }

    pub fn size_bytes(&self) -> usize {
        self.nodes
            .iter()
            .map(|n| match n {
                Node::Leaf(_) => 1,
                Node::Inner { bv, .. } => bv.size_bytes() + 8,
            })
            .sum::<usize>()
            + 256 * 9 // 符号表
    }

    /// 平均符号長(ビット)。索引サイズの見通しに使う
    pub fn mean_code_len(&self, text: &[u8]) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let total: u64 = text.iter().map(|&b| self.codes[b as usize].1 as u64).sum();
        total as f64 / text.len() as f64
    }
}

// ---------------------------------------------------------------- 直列化

/// 配信形式では、rank 標本(常駐させたい)とワード列(必要な分だけ取りに行く)を
/// 別の領域に置く。木の形と符号表は「メタ」として常駐側に置く。
impl WaveletTree {
    pub fn write_parts(&self, meta: &mut Vec<u8>, sb: &mut Vec<u8>, words: &mut Vec<u8>) {
        meta.extend_from_slice(&(self.len as u64).to_le_bytes());
        meta.extend_from_slice(&(self.nodes.len() as u32).to_le_bytes());
        for n in &self.nodes {
            match n {
                Node::Leaf(s) => {
                    meta.push(0);
                    meta.push(*s);
                }
                Node::Inner { bv, left, right } => {
                    meta.push(1);
                    meta.extend_from_slice(&left.to_le_bytes());
                    meta.extend_from_slice(&right.to_le_bytes());
                    meta.extend_from_slice(&(bv.len() as u64).to_le_bytes());
                    meta.extend_from_slice(&(bv.superblocks().len() as u32).to_le_bytes());
                    meta.extend_from_slice(&(bv.words().len() as u32).to_le_bytes());
                    for &v in bv.superblocks() {
                        sb.extend_from_slice(&v.to_le_bytes());
                    }
                    for &w in bv.words() {
                        words.extend_from_slice(&w.to_le_bytes());
                    }
                }
            }
        }
        for (code, len) in self.codes.iter() {
            meta.extend_from_slice(&code.to_le_bytes());
            meta.push(*len);
        }
    }

    pub fn read_parts(meta: &[u8], sb: &[u8], words: &[u8]) -> Self {
        let mut p = 0usize;
        let u64at = |p: &mut usize| {
            let v = u64::from_le_bytes(meta[*p..*p + 8].try_into().unwrap());
            *p += 8;
            v
        };
        let len = u64at(&mut p) as usize;
        let n_nodes = {
            let v = u32::from_le_bytes(meta[p..p + 4].try_into().unwrap());
            p += 4;
            v as usize
        };
        let mut nodes = Vec::with_capacity(n_nodes);
        let (mut sb_at, mut w_at) = (0usize, 0usize);
        for _ in 0..n_nodes {
            let tag = meta[p];
            p += 1;
            if tag == 0 {
                nodes.push(Node::Leaf(meta[p]));
                p += 1;
            } else {
                let left = u32::from_le_bytes(meta[p..p + 4].try_into().unwrap());
                let right = u32::from_le_bytes(meta[p + 4..p + 8].try_into().unwrap());
                p += 8;
                let bv_len = u64::from_le_bytes(meta[p..p + 8].try_into().unwrap()) as usize;
                p += 8;
                let n_sb = u32::from_le_bytes(meta[p..p + 4].try_into().unwrap()) as usize;
                let n_w = u32::from_le_bytes(meta[p + 4..p + 8].try_into().unwrap()) as usize;
                p += 8;
                let sbs: Vec<u32> = (0..n_sb)
                    .map(|i| {
                        u32::from_le_bytes(sb[sb_at + i * 4..sb_at + i * 4 + 4].try_into().unwrap())
                    })
                    .collect();
                sb_at += n_sb * 4;
                let ws: Vec<u64> = (0..n_w)
                    .map(|i| {
                        u64::from_le_bytes(
                            words[w_at + i * 8..w_at + i * 8 + 8].try_into().unwrap(),
                        )
                    })
                    .collect();
                w_at += n_w * 8;
                nodes.push(Node::Inner {
                    bv: BitVec::from_parts(ws, sbs, bv_len),
                    left,
                    right,
                });
            }
        }
        let mut codes = [(0u64, 0u8); 256];
        for slot in codes.iter_mut() {
            let code = u64::from_le_bytes(meta[p..p + 8].try_into().unwrap());
            p += 8;
            *slot = (code, meta[p]);
            p += 1;
        }
        WaveletTree { nodes, codes, len }
    }

    /// 常駐領域(rank 標本)と随時取得領域(ワード列)の大きさ
    pub fn region_bytes(&self) -> (usize, usize) {
        let mut sb = 0;
        let mut w = 0;
        for n in &self.nodes {
            if let Node::Inner { bv, .. } = n {
                sb += bv.superblock_bytes();
                w += bv.word_bytes();
            }
        }
        (sb, w)
    }
}
