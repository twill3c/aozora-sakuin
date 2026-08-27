// 本番のヘルスチェック。Range で全件を数え、手元の実測値と一致することを見る。
//
//   node scripts/verify_prod.mjs [URL]

const BASE = process.argv[2] || "https://aozora-sakuin.vercel.app/";
const EXPECT = { "あはれ": 1197, "吾輩": 1146 };   // 手元での実測(全 5,000 作)
const enc = new TextEncoder();

const LIMIT = 16;
let active = 0; const waiting = [];
const slot = async () => { if (active >= LIMIT) await new Promise((r) => waiting.push(r)); active++; };
const release = () => { active--; const n = waiting.shift(); if (n) n(); };

let sent = 0, reqs = 0;
async function getRange(file, at, len) {
  await slot();
  try {
    const r = await fetch(BASE + "index/" + file, { headers: { Range: `bytes=${at}-${at + len - 1}` } });
    if (r.status !== 206) throw new Error(`${file} が Range に応じない (${r.status})`);
    const b = new Uint8Array(await r.arrayBuffer());
    sent += b.length; reqs++;
    return b;
  } finally { release(); }
}

const wasmBytes = new Uint8Array(await (await fetch(BASE + "wasm/sakuin.wasm")).arrayBuffer());
const manifest = await (await fetch(BASE + "index/manifest.json")).json();
const { instance } = await WebAssembly.instantiate(wasmBytes, {});
const az = instance.exports;
const mem = () => new Uint8Array(az.memory.buffer);
const put = (b) => { const p = az.az_alloc(b.length); mem().set(b, p); return p; };

if (az.az_format_version() !== manifest.format_version) {
  throw new Error(`版が食い違う: 索引 ${manifest.format_version} / 照合器 ${az.az_format_version()}`);
}
const whole = manifest.shards.reduce((a, s) => a + s.bytes, 0);
console.log(`${BASE}`);
console.log(`索引 v${manifest.format_version} / ${manifest.shards.length} 枚 / ${(whole / 1e6).toFixed(1)} MB / 収録 ${manifest.works.toLocaleString()} 作`);

let t0 = Date.now();
const handles = [];
await Promise.all(manifest.shards.map(async (s, i) => {
  const buf = await getRange(s.file, 0, s.resident);
  const p = put(buf);
  handles[i] = az.az_resident_load(p, buf.length);
  az.az_free(p, buf.length);
  if (handles[i] < 0) throw new Error(`${s.file} の常駐領域を読めない`);
}));
const residentMs = Date.now() - t0, residentBytes = sent;
console.log(`索引の目次 ${(residentBytes / 1e6).toFixed(1)} MB / ${reqs} 要求 / ${residentMs}ms`);

console.log(`\n${"語".padEnd(10)}${"件数".padStart(9)}${"期待".padStart(9)}${"追加転送".padStart(11)}${"要求".padStart(8)}${"所要".padStart(9)}`);
let ok = true;
for (const [word, expect] of Object.entries(EXPECT)) {
  const w = enc.encode(word);
  const before = { sent, reqs }; const t = Date.now();
  const counts = await Promise.all(manifest.shards.map(async (s, i) => {
    const wp = put(w);
    try {
      for (let round = 0; round < 500; round++) {
        const r = az.az_resident_count(handles[i], wp, w.length);
        if (r >= 0) return r;
        if (r !== -2) throw new Error(`${s.file} で戻り値 ${r}`);
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
      throw new Error("収束しない");
    } finally { az.az_free(wp, w.length); }
  }));
  const total = counts.reduce((a, b) => a + b, 0);
  const good = total === expect;
  ok &&= good;
  console.log(`${word.padEnd(10)}${String(total).padStart(9)}${String(expect).padStart(9)}${((sent - before.sent) / 1e6).toFixed(2).padStart(9)}MB${String(reqs - before.reqs).padStart(8)}${((Date.now() - t) + "ms").padStart(9)}  ${good ? "一致" : "★不一致★"}`);
}
if (!ok) throw new Error("本番の件数が手元の実測と一致しない");
console.log(`\n総転送 ${(sent / 1e6).toFixed(1)} MB / 索引全体 ${(whole / 1e6).toFixed(1)} MB (${(whole / sent).toFixed(0)} 分の 1)`);
console.log("OK — 本番で Range が効き、全 5,000 作の件数が手元と一致");
