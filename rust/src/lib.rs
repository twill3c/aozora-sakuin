//! 青空索引 — 5,000 作の全文索引。
//!
//! ## なぜ Rust か
//!
//! 速度のためではない。正味 70 MB の本文に対する素の接尾辞配列は 4 バイト整数 × n =
//! 280 MB になり、ブラウザへ配信できない。配信できる大きさに収めるには BWT と
//! 疎な SA 標本による圧縮索引が要り、その構築(SA-IS)と照合(rank/select の
//! ビット演算)は JavaScript では現実的に書けない。
//!
//! ## 正しさの担保
//!
//! すべての索引操作に総当たりの参照実装を置き、両者の一致を検査する(二実装照合)。
//! 圧縮索引の内部が自己無矛盾であることは、解釈が正しいことを意味しない(HC-027)。

pub mod bitvec;
pub mod bwt;
pub mod fm;
pub mod sais;
pub mod shard;
pub mod variants;
pub mod wasm;
pub mod wavelet;
