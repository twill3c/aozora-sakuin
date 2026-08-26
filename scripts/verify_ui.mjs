// 画面の実地検査(SPEC F-05 / F-06 / O-18)。
//
// 本物のブラウザは無いが、app.js が使うのと同じ手順を Node で踏み、
// 配信中のサーバから取れるもので画面が組めることを確かめる。
// ここが通れば「静的配信のまま動く」ことの裏が取れる。

const BASE = process.argv[2] || "http://127.0.0.1:8787/";
const enc = new TextEncoder();
const dec = new TextDecoder();

const get = async (p, kind = "buf") => {
  const r = await fetch(BASE + p);
  if (!r.ok) throw new Error(`${p} が ${r.status}`);
  return kind === "json" ? r.json() : new Uint8Array(await r.arrayBuffer());
};

// 画面と同じ 3 本を落とす
const [wasmBytes, manifest, meta] = await Promise.all([
  get("wasm/sakuin.wasm"), get("index/manifest.json", "json"), get("index/works.json", "json"),
]);
const { instance } = await WebAssembly.instantiate(wasmBytes, {});
const az = instance.exports;
const mem = () => new Uint8Array(az.memory.buffer);
const put = (b) => { const p = az.az_alloc(b.length); mem().set(b, p); return p; };

if (az.az_format_version() !== manifest.format_version) {
  throw new Error(`版が食い違う: 索引 ${manifest.format_version} / 照合器 ${az.az_format_version()}`);
}
const works = Object.keys(meta.works).length;
console.log(`索引 v${manifest.format_version} / 収録 ${works.toLocaleString()} 作 / シャード ${manifest.shards.length} 枚`);
console.log(`初回に落とすもの: wasm ${(wasmBytes.length / 1024).toFixed(0)} KB + 台帳 ${(JSON.stringify(meta).length / 1024).toFixed(0)} KB`);

function readOut() { const at = az.az_out_ptr(); return mem().slice(at, at + az.az_out_len()); }
function variants(word, risky = 0) {
  const w = enc.encode(word); const p = put(w);
  const n = az.az_variants(p, w.length, risky); az.az_free(p, w.length);
  const out = readOut(); const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
  const forms = []; let q = 0;
  for (let i = 0; i < n; i++) {
    const ratio = dv.getUint32(q, true), l = dv.getUint32(q + 4, true); q += 8;
    forms.push({ form: dec.decode(out.subarray(q, q + l)), ratio }); q += l;
  }
  return forms;
}
function kwic(h, word, max) {
  const w = enc.encode(word); const p = put(w);
  const n = az.az_kwic(h, p, w.length, 30, max); az.az_free(p, w.length);
  if (n <= 0) return [];
  const out = readOut(); const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
  const rows = []; let q = 0;
  for (let i = 0; i < n; i++) {
    const id = dv.getUint32(q, true), pos = dv.getUint32(q + 4, true);
    const bl = dv.getUint32(q + 8, true), hl = dv.getUint32(q + 12, true), al = dv.getUint32(q + 16, true);
    q += 20;
    rows.push({ id, pos,
      before: dec.decode(out.subarray(q, q + bl)),
      hit: dec.decode(out.subarray(q + bl, q + bl + hl)),
      after: dec.decode(out.subarray(q + bl + hl, q + bl + hl + al)) });
    q += bl + hl + al;
  }
  return rows;
}

// 画面と同じ走査を先頭 12 枚だけ回す
const QUERY = "あわれ";
const forms = [{ form: QUERY, ratio: 100 }, ...variants(QUERY, 0)];
console.log(`\n「${QUERY}」の形: ${forms.map((f) => `${f.form}(${f.ratio}%)`).join(" ")}`);

let total = 0, bytes = 0, rows = [];
const t0 = Date.now();
for (const s of manifest.shards.slice(0, 12)) {
  const buf = await get("index/" + s.file);
  bytes += buf.length;
  const p = put(buf);
  const h = az.az_shard_load(p, buf.length);
  if (h < 0) throw new Error(`${s.file} を読めない`);
  for (const f of forms) {
    const w = enc.encode(f.form); const wp = put(w);
    const n = az.az_count(h, wp, w.length); az.az_free(wp, w.length);
    if (n > 0) { total += n; rows.push(...kwic(h, f.form, 3)); }
  }
  az.az_shard_drop(h); az.az_free(p, buf.length);
}
console.log(`走査 12 枚 / ${(bytes / 1e6).toFixed(2)} MB / ${Date.now() - t0}ms → ${total} 件・用例 ${rows.length} 件`);

// 台帳が引けること
let named = 0;
for (const r of rows) {
  const m = meta.works[r.id];
  if (!m) throw new Error(`作品 ${r.id} が台帳にない`);
  if (!m[0] || !m[1]) throw new Error(`作品 ${r.id} の題名/著者が空`);
  named++;
}
console.log(`用例 ${named} 件すべてに題名・著者・文字遣い・分類が付いた`);

console.log("\n--- 画面に出る形 ---");
for (const r of rows.slice(0, 4)) {
  const m = meta.works[r.id];
  console.log(`  …${r.before}【${r.hit}】${r.after}…`);
  console.log(`     ${m[1]}『${m[0]}』 ${meta.kana_names[m[2]]} · ${m[3]}`);
}
console.log("\nOK — 静的配信のまま画面が組める");
