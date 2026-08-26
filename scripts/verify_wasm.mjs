// wasm 境界の実地検査(SPEC O-14)。
//
// 同じ問いを (1) 実際の wasm と (2) 素朴な全文検索 に投げ、答えが一致することを見る。
// ここが通れば「ブラウザで動く形」まで到達している。JS 側が知っているのは
// レコードの並びだけで、索引の構造には一切触れていない。

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const wasmBytes = readFileSync(join(ROOT, "web/wasm/sakuin.wasm"));
const { instance } = await WebAssembly.instantiate(wasmBytes, {});
const az = instance.exports;
const mem = () => new Uint8Array(az.memory.buffer);

const enc = new TextEncoder();
const dec = new TextDecoder();

function put(bytes) {
  const p = az.az_alloc(bytes.length);
  mem().set(bytes, p);
  return p;
}

function kwic(h, word, width, max) {
  const w = enc.encode(word);
  const wp = put(w);
  const n = az.az_kwic(h, wp, w.length, width, max);
  az.az_free(wp, w.length);
  if (n < 0) throw new Error("az_kwic が失敗した");
  const out = mem().slice(az.az_out_ptr(), az.az_out_ptr() + az.az_out_len());
  const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
  const recs = [];
  let p = 0;
  for (let i = 0; i < n; i++) {
    const id = dv.getUint32(p, true);
    const pos = dv.getUint32(p + 4, true);
    const bl = dv.getUint32(p + 8, true);
    const hl = dv.getUint32(p + 12, true);
    const al = dv.getUint32(p + 16, true);
    p += 20;
    recs.push({
      id, pos,
      before: dec.decode(out.subarray(p, p + bl)),
      hit: dec.decode(out.subarray(p + bl, p + bl + hl)),
      after: dec.decode(out.subarray(p + bl + hl, p + bl + hl + al)),
    });
    p += bl + hl + al;
  }
  return recs;
}

function count(h, word) {
  const w = enc.encode(word);
  const wp = put(w);
  const n = az.az_count(h, wp, w.length);
  az.az_free(wp, w.length);
  return n;
}

// 表記ゆれの展開 — 規則も較正値も Rust 側にあり、JS は形の並びを受け取るだけ
function variants(word, risky = 0) {
  const w = enc.encode(word);
  const wp = put(w);
  const n = az.az_variants(wp, w.length, risky);
  az.az_free(wp, w.length);
  if (n < 0) throw new Error("az_variants が失敗した");
  const out = mem().slice(az.az_out_ptr(), az.az_out_ptr() + az.az_out_len());
  const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
  const forms = [];
  let p = 0;
  for (let i = 0; i < n; i++) {
    const ratio = dv.getUint32(p, true);
    const len = dv.getUint32(p + 4, true);
    p += 8;
    forms.push({ form: dec.decode(out.subarray(p, p + len)), ratio });
    p += len;
  }
  return forms;
}

// --- 実データで検査 ---
const dir = join(ROOT, "web/index");
const files = readdirSync(dir).filter((f) => f.endsWith(".azsk")).sort();
console.log(`配信形式の版 ${az.az_format_version()} / シャード ${files.length} 枚`);

const WORDS = ["あはれ", "うつくしい", "吾輩", "東京", "こころ"];
const totals = Object.fromEntries(WORDS.map((w) => [w, 0]));
let docs = 0;
let checked = 0;
let t0 = Date.now();

// 総当たりの照合相手 — 正規化本文をそのまま読む
const normDir = join(ROOT, "data/normalized");

for (const f of files.slice(0, 6)) {          // 実地検査は先頭 6 枚に絞る
  const buf = readFileSync(join(dir, f));
  const p = put(buf);
  const h = az.az_shard_load(p, buf.length);
  if (h < 0) throw new Error(`${f} を読めない`);
  docs += az.az_shard_docs(h);

  for (const w of WORDS) totals[w] += count(h, w);

  // 用例を本文と突き合わせる。頻出語は全件だと重いので上限を置く
  const bodies = new Map();
  for (const w of WORDS) {
    const recs = kwic(h, w, 20, 400);
    // 各用例が、その作品の本文にそのまま現れること
    for (const r of recs) {
      if (!bodies.has(r.id)) {
        bodies.set(r.id, readFileSync(join(normDir, String(r.id).padStart(6, "0") + ".txt"), "utf8"));
      }
      const body = bodies.get(r.id);
      const whole = r.before + r.hit + r.after;
      if (!body.includes(whole)) throw new Error(`作品 ${r.id} に文脈が無い: ${whole}`);
      if (r.hit !== w) throw new Error(`一致部が違う: ${r.hit} != ${w}`);
      checked++;
    }
    // 上限に達していなければ、件数と用例数が一致する
    const n = count(h, w);
    if (recs.length !== Math.min(n, 400)) {
      throw new Error(`${w} の件数 ${n} と用例数 ${recs.length} が食い違う`);
    }
  }
  az.az_shard_drop(h);
  az.az_free(p, buf.length);
}

console.log(`検査したシャード 6 枚 / 作品 ${docs} 作 / 用例 ${checked} 件 / ${Date.now() - t0}ms`);
console.log("件数(先頭 6 枚):", totals);

// 見た目の確認
const buf = readFileSync(join(dir, files[0]));
const p = put(buf);
const h = az.az_shard_load(p, buf.length);
console.log("\n--- wasm 越しの用例 ---");
for (const r of kwic(h, "あはれ", 26, 3)) {
  console.log(`  …${r.before}【${r.hit}】${r.after}…`);
  console.log(`     作品 ${r.id} / ${r.pos} 字目`);
}

// --- 表記ゆれの実地確認 ---
console.log("\n--- wasm 越しの表記ゆれ展開 ---");
for (const w of ["あわれ", "ように", "しずか"]) {
  const forms = variants(w);
  const base = count(h, w);
  let extra = 0;
  for (const f of forms) extra += count(h, f.form);
  console.log(`  「${w}」→ ${forms.map((f) => `${f.form}(${f.ratio}%)`).join(" ")}`);
  console.log(`      このシャードで ${base} 件 → ${base + extra} 件`);
}
{
  const plain = variants("おもう").map((f) => f.form);
  const risky = variants("おもう", 1).map((f) => f.form);
  if (plain.includes("をもう")) throw new Error("衝突しやすい形が既定で出ている");
  if (!risky.includes("をもう")) throw new Error("明示しても衝突しやすい形が出ない");
  console.log(`  「おもう」既定 ${plain.length} 形 / 衝突込み ${risky.length} 形`);
}
az.az_shard_drop(h);

console.log("\nOK — wasm 越しの結果が本文と一致");
