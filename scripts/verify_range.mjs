// Range 経路の実地検査(SPEC O-03)。
//
// 実際の HTTP Range で必要なバイトだけ取り、全 228 枚の件数が
// 丸ごと読んだ場合と一致することを確かめる。転送量と所要時間も測る。

const BASE = process.argv[2] || "http://127.0.0.1:8791/";
const enc = new TextEncoder();
const WORDS = process.argv.slice(3).length ? process.argv.slice(3)
  : ["あはれ", "うつくしい", "吾輩", "東京"];

let sent = 0, reqs = 0;

// ブラウザも同時接続数を絞る。無制限に投げると配信側の待ち行列が溢れる
const LIMIT = 24;
let active = 0;
const waiting = [];
async function slot() {
  if (active >= LIMIT) await new Promise((r) => waiting.push(r));
  active++;
}
function release() {
  active--;
  const next = waiting.shift();
  if (next) next();
}
async function getRange(file, at, len) {
  await slot();
  try {
    const r = await fetch(BASE + "index/" + file, {
      headers: { Range: `bytes=${at}-${at + len - 1}` },
    });
    if (r.status !== 206) throw new Error(`${file} が Range に応じない (${r.status})`);
    const b = new Uint8Array(await r.arrayBuffer());
    sent += b.length; reqs++;
    return b;
  } finally {
    release();
  }
}
async function getWhole(file) {
  const b = new Uint8Array(await (await fetch(BASE + "index/" + file)).arrayBuffer());
  return b;
}

const wasmBytes = new Uint8Array(await (await fetch(BASE + "wasm/sakuin.wasm")).arrayBuffer());
const manifest = await (await fetch(BASE + "index/manifest.json")).json();
const { instance } = await WebAssembly.instantiate(wasmBytes, {});
const az = instance.exports;
const mem = () => new Uint8Array(az.memory.buffer);
const put = (b) => { const p = az.az_alloc(b.length); mem().set(b, p); return p; };

const whole = manifest.shards.reduce((a, s) => a + s.bytes, 0);
const residentTotal = manifest.shards.reduce((a, s) => a + s.resident, 0);
console.log(`シャード ${manifest.shards.length} 枚 / 索引 ${(whole / 1e6).toFixed(1)} MB / 常駐 ${(residentTotal / 1e6).toFixed(1)} MB`);

// --- 常駐領域を読み込む(初回のみ) ---
let t0 = Date.now();
const handles = [];
await Promise.all(manifest.shards.map(async (s, i) => {
  const buf = await getRange(s.file, 0, s.resident);
  const p = put(buf);
  const h = az.az_resident_load(p, buf.length);
  if (h < 0) throw new Error(`${s.file} の常駐領域を読めない`);
  handles[i] = h;
}));
console.log(`常駐領域 ${manifest.shards.length} 枚 / ${(sent / 1e6).toFixed(1)} MB / ${reqs} 要求 / ${Date.now() - t0}ms`);

// --- Range で数える ---
async function countAll(word) {
  const w = enc.encode(word);
  const before = { sent, reqs };
  const t = Date.now();
  let maxRounds = 0;
  const counts = await Promise.all(manifest.shards.map(async (s, i) => {
    const wp = put(w);
    let rounds = 0;
    for (;;) {
      const r = az.az_resident_count(handles[i], wp, w.length);
      if (r >= 0) { az.az_free(wp, w.length); maxRounds = Math.max(maxRounds, rounds); return r; }
      if (r !== -2) throw new Error(`${s.file} で戻り値 ${r}`);
      rounds++;
      if (rounds > 500) throw new Error(`${s.file} が収束しない`);
      const out = mem().slice(az.az_out_ptr(), az.az_out_ptr() + az.az_out_len());
      const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
      for (let k = 0; k + 8 <= out.length; k += 8) {
        const at = dv.getUint32(k, true), len = dv.getUint32(k + 4, true);
        const bytes = await getRange(s.file, at, len);
        const bp = put(bytes);
        az.az_resident_supply(handles[i], at, bp, bytes.length);
        az.az_free(bp, bytes.length);
      }
    }
  }));
  return {
    total: counts.reduce((a, b) => a + b, 0),
    ms: Date.now() - t,
    bytes: sent - before.sent,
    reqs: reqs - before.reqs,
    maxRounds,
  };
}

console.log(`\n${"語".padEnd(12)}${"件数".padStart(10)}${"往復(最大)".padStart(12)}${"追加転送".padStart(11)}${"要求数".padStart(9)}${"所要".padStart(9)}`);
const results = {};
for (const w of WORDS) {
  const r = await countAll(w);
  results[w] = r.total;
  console.log(`${w.padEnd(12)}${String(r.total).padStart(10)}${String(r.maxRounds).padStart(12)}${(r.bytes / 1e6).toFixed(2).padStart(9)}MB${String(r.reqs).padStart(9)}${(r.ms + "ms").padStart(9)}`);
}

// --- 丸ごと読みと照合(先頭 20 枚で確かめる) ---
console.log("\n--- 丸ごと読みとの照合(先頭 20 枚) ---");
for (const w of WORDS.slice(0, 2)) {
  const wbytes = enc.encode(w);
  let a = 0, b = 0;
  for (const s of manifest.shards.slice(0, 20)) {
    const buf = await getWhole(s.file);
    const p = put(buf);
    const h = az.az_shard_load(p, buf.length);
    const wp = put(wbytes);
    a += az.az_count(h, wp, wbytes.length);
    az.az_free(wp, wbytes.length);
    az.az_shard_drop(h); az.az_free(p, buf.length);
  }
  // 同じ 20 枚を Range で
  for (let i = 0; i < 20; i++) {
    const wp = put(wbytes);
    for (;;) {
      const r = az.az_resident_count(handles[i], wp, wbytes.length);
      if (r >= 0) { b += r; break; }
      const out = mem().slice(az.az_out_ptr(), az.az_out_ptr() + az.az_out_len());
      const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
      for (let k = 0; k + 8 <= out.length; k += 8) {
        const at = dv.getUint32(k, true), len = dv.getUint32(k + 4, true);
        const bytes = await getRange(manifest.shards[i].file, at, len);
        const bp = put(bytes);
        az.az_resident_supply(handles[i], at, bp, bytes.length);
        az.az_free(bp, bytes.length);
      }
    }
    az.az_free(wp, wbytes.length);
  }
  if (a !== b) throw new Error(`「${w}」 丸ごと ${a} / Range ${b} が一致しない`);
  console.log(`  ${w}: 丸ごと ${a} = Range ${b}  一致`);
}

console.log(`\n総転送 ${(sent / 1e6).toFixed(1)} MB / 索引全体 ${(whole / 1e6).toFixed(1)} MB (${(whole / sent).toFixed(0)} 分の 1)`);
console.log("OK — Range で必要なバイトだけ読んで全件を数えられる");
