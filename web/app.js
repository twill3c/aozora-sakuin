// 青空索引 — 店先(表示とグルー)。
//
// ここには索引の知識を置かない(SPEC N-01 / N-02)。JS が知っているのは
//   1. シャードのバイト列を wasm に渡すこと
//   2. 語を渡すこと
//   3. 返ってきたレコードの並びをほどくこと
// の 3 つだけで、BWT もウェーブレット木も LF 写像も境界の向こう側にある。
//
// レコードの並び:
//   用例   u32 作品ID / u32 作品内位置 / u32 前文脈長 / u32 一致長 / u32 後文脈長 / 本文
//   異体形 u32 旧仮名率 / u32 長さ / バイト列

const BASE = "index/";
const WIDTH = 30;          // 前後文脈の長さ(バイト)
const PER_SHARD = 6;       // 1 シャードから拾う用例の上限
const SHOW_MAX = 120;      // 画面に出す用例の上限

const $ = (id) => document.getElementById(id);
const enc = new TextEncoder();
const dec = new TextDecoder();
const fmt = (n) => n.toLocaleString("en-US");

let az = null;             // wasm の輸出
let manifest = null;
let meta = null;           // 作品台帳
let kanaNames = [];
const residents = [];      // シャードごとの取っ手(常駐領域)。初回に読んで使い回す

const state = {
  query: "",
  forms: [],               // [{form, ratio, base}]
  counts: new Map(),       // form -> 件数
  rows: [],                // 用例
  byWork: new Map(),       // 作品ID -> 件数
  perShard: new Map(),     // シャード番号 -> {形: 件数}
  scanned: 0,
  bytes: 0,
  total: 0,
  running: false,
  facets: { kana: new Set(), genre: new Set() },
};

// ---------------------------------------------------------------- wasm

function mem() { return new Uint8Array(az.memory.buffer); }

function put(bytes) {
  const p = az.az_alloc(bytes.length);
  mem().set(bytes, p);
  return p;
}

function withBytes(bytes, fn) {
  const p = put(bytes);
  try { return fn(p, bytes.length); } finally { az.az_free(p, bytes.length); }
}

function readOut() {
  const at = az.az_out_ptr();
  return mem().slice(at, at + az.az_out_len());
}

function variantsOf(word, risky) {
  return withBytes(enc.encode(word), (p, len) => {
    const n = az.az_variants(p, len, risky ? 1 : 0);
    if (n < 0) return [];
    const out = readOut();
    const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
    const forms = [];
    let q = 0;
    for (let i = 0; i < n; i++) {
      const ratio = dv.getUint32(q, true);
      const l = dv.getUint32(q + 4, true);
      q += 8;
      forms.push({ form: dec.decode(out.subarray(q, q + l)), ratio, base: false });
      q += l;
    }
    return forms;
  });
}

function countIn(h, word) {
  return withBytes(enc.encode(word), (p, len) => az.az_count(h, p, len));
}

function kwicIn(h, word, max) {
  return withBytes(enc.encode(word), (p, len) => {
    const n = az.az_kwic(h, p, len, WIDTH, max);
    if (n <= 0) return [];
    const out = readOut();
    const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
    const rows = [];
    let q = 0;
    for (let i = 0; i < n; i++) {
      const id = dv.getUint32(q, true);
      const pos = dv.getUint32(q + 4, true);
      const bl = dv.getUint32(q + 8, true);
      const hl = dv.getUint32(q + 12, true);
      const al = dv.getUint32(q + 16, true);
      q += 20;
      rows.push({
        id, pos, word,
        before: dec.decode(out.subarray(q, q + bl)),
        hit: dec.decode(out.subarray(q + bl, q + bl + hl)),
        after: dec.decode(out.subarray(q + bl + hl, q + bl + hl + al)),
      });
      q += bl + hl + al;
    }
    return rows;
  });
}

// ---------------------------------------------------------------- 走査
//
// 二段構え。
//   1. 件数 — 各シャードの常駐領域(索引全体の 5%)を読んでおき、足りないバイトだけ
//      Range で取りに行く。全件が正確に出て、追加転送は 1 クエリ 0.1 MB 程度
//   2. 用例 — 件のあるシャードだけ丸ごと落として前後文脈を組む
//
// 位置復元と文脈取り出しは LF を数百段も逐次に辿るので Range では往復が過大になる。
// 数えるのと見せるので経路を分けている。

const LIMIT = 12;               // 同時に投げる要求の数
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
    const r = await fetch(BASE + file, { headers: { Range: `bytes=${at}-${at + len - 1}` } });
    // 206 以外は受けない。200 が返るのは配信側が Range を無視してファイル全体を
    // 返した場合で、そのまま供給すると別のバイトを索引に食わせることになる。
    // 静かに壊れるより落ちるほうがよい
    if (r.status !== 206) {
      throw new Error(`${file} が Range に応じません(${r.status})。配信側が Range 未対応です`);
    }
    const b = new Uint8Array(await r.arrayBuffer());
    state.bytes += b.length;
    return b;
  } finally {
    release();
  }
}

/// 常駐領域を読み込む(初回のみ)。以後のクエリはこれを使い回す
async function ensureResidents(onStep) {
  if (residents.length === manifest.shards.length) return;
  residents.length = 0;
  let done = 0;
  await Promise.all(manifest.shards.map(async (s, i) => {
    const buf = await getRange(s.file, 0, s.resident);
    const p = put(buf);
    const h = az.az_resident_load(p, buf.length);
    az.az_free(p, buf.length);
    if (h < 0) throw new Error(`${s.file} の常駐領域を読めない`);
    residents[i] = h;
    onStep(++done);
  }));
}

/// 1 シャードで 1 つの形を数える。足りないバイトは取りに行く
async function countOne(i, wordBytes) {
  const wp = put(wordBytes);
  try {
    for (let round = 0; round < 500; round++) {
      const r = az.az_resident_count(residents[i], wp, wordBytes.length);
      if (r >= 0) return r;
      if (r !== -2) throw new Error(`戻り値 ${r}`);
      const out = readOut();
      const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
      for (let k = 0; k + 8 <= out.length; k += 8) {
        const at = dv.getUint32(k, true);
        const len = dv.getUint32(k + 4, true);
        const bytes = await getRange(manifest.shards[i].file, at, len);
        const bp = put(bytes);
        az.az_resident_supply(residents[i], at, bp, bytes.length);
        az.az_free(bp, bytes.length);
      }
    }
    throw new Error("収束しない");
  } finally {
    az.az_free(wp, wordBytes.length);
  }
}

/// 用例を組む。件のあるシャードを丸ごと落とす
async function examplesFrom(i, forms) {
  await slot();
  let buf;
  try {
    buf = new Uint8Array(await (await fetch(BASE + manifest.shards[i].file)).arrayBuffer());
  } finally {
    release();
  }
  state.bytes += buf.length;
  const p = put(buf);
  const h = az.az_shard_load(p, buf.length);
  if (h < 0) { az.az_free(p, buf.length); return; }
  for (const f of forms) {
    if (state.rows.length >= SHOW_MAX) break;
    if ((state.perShard.get(i) || {})[f.form]) {
      for (const r of kwicIn(h, f.form, PER_SHARD)) {
        state.rows.push(r);
        state.byWork.set(r.id, (state.byWork.get(r.id) || 0) + 1);
      }
    }
  }
  az.az_shard_drop(h);
  az.az_free(p, buf.length);
}

async function search() {
  const q = $("q").value.trim();
  if (!q || !az || state.running) return;
  const useVariants = $("opt-variants").getAttribute("aria-pressed") === "true";
  const risky = $("opt-risky").getAttribute("aria-pressed") === "true";

  state.query = q;
  state.forms = [{ form: q, ratio: 100, base: true }];
  if (useVariants) state.forms.push(...variantsOf(q, risky));
  state.counts = new Map();
  state.rows = [];
  state.byWork = new Map();
  state.perShard = new Map();
  state.total = 0;
  state.scanned = 0;
  state.bytes = 0;
  state.running = true;
  state.facets = { kana: new Set(), genre: new Set() };
  $("readout").hidden = false;
  $("empty").hidden = true;
  $("run").disabled = true;
  $("phase").textContent = "索引の目次を読み込み中…";
  render();

  try {
    await ensureResidents((n) => {
      state.scanned = n;
      $("phase").textContent = `索引の目次 ${n} / ${manifest.shards.length} 枚`;
      render();
    });

    // 第 1 段 — 全件を数える
    $("phase").textContent = "全件を数えています…";
    state.scanned = 0;
    const encoded = state.forms.map((f) => ({ f, b: enc.encode(f.form) }));
    await Promise.all(manifest.shards.map(async (_, i) => {
      const here = {};
      for (const { f, b } of encoded) {
        const n = await countOne(i, b);
        if (n > 0) {
          here[f.form] = n;
          state.counts.set(f.form, (state.counts.get(f.form) || 0) + n);
          state.total += n;
        }
      }
      if (Object.keys(here).length) state.perShard.set(i, here);
      state.scanned++;
      if (state.scanned % 4 === 0) render();
    }));
    render();

    // 第 2 段 — 用例を組む
    const withHits = [...state.perShard.keys()].sort((a, b) => a - b);
    $("phase").textContent = `用例を組んでいます(該当 ${withHits.length} 枚)…`;
    for (const i of withHits) {
      if (state.rows.length >= SHOW_MAX) break;
      await examplesFrom(i, state.forms);
      render();
    }
    $("phase").textContent = "";
  } catch (e) {
    $("phase").textContent = `失敗しました: ${e.message}`;
    console.error(e);
  } finally {
    state.running = false;
    $("run").disabled = false;
    render();
  }
}

// ---------------------------------------------------------------- 表示

function visibleRows() {
  const { kana, genre } = state.facets;
  return state.rows.filter((r) => {
    const m = meta.works[r.id];
    if (!m) return false;
    if (kana.size && !kana.has(m[2])) return false;
    if (genre.size && !genre.has(m[3])) return false;
    return true;
  });
}

function render() {
  $("hits").textContent = fmt(state.total);
  $("scan").textContent = state.scanned;
  $("scan-total").textContent = manifest.shards.length;
  $("bytes").textContent = (state.bytes / 1e6).toFixed(1);
  $("progress-fill").style.width =
    `${(state.scanned / manifest.shards.length * 100).toFixed(1)}%`;
  $("hit-shards").textContent = state.perShard.size;

  // 当たった形
  const fw = $("forms");
  fw.innerHTML = "";
  for (const f of state.forms) {
    const n = f.base ? (state.counts.get(f.form) || 0) : (state.counts.get(f.form) || 0);
    const el = document.createElement("div");
    el.className = "form" + (!f.base && f.ratio < 70 ? " risky" : "");
    el.innerHTML = '<span class="w"></span><span class="n"></span><span class="why"></span>';
    el.querySelector(".w").textContent = f.form;
    if (f.base) el.querySelector(".w").classList.add("base");
    el.querySelector(".n").textContent = fmt(n);
    el.querySelector(".why").textContent = f.base
      ? "入力した語"
      : `旧仮名の作品に落ちる割合 ${f.ratio}%`;
    fw.appendChild(el);
  }

  const rows = visibleRows();
  $("shown").textContent = fmt(rows.length);

  // 絞り込み
  facet("facet-kana", "kana", countBy(state.rows, (m) => m[2]), (k) => kanaNames[k]);
  facet("facet-genre", "genre", countBy(state.rows, (m) => m[3]), (k) => k);

  // 用例
  const kw = $("kwic");
  kw.innerHTML = "";
  for (const r of rows) {
    const m = meta.works[r.id] || ["(不明)", "", 4, "", 0];
    const el = document.createElement("article");
    el.className = "row";
    el.innerHTML =
      '<div class="before"></div><div class="hit"></div><div class="after"></div>' +
      '<div class="src"><span class="work"><em></em>『<span class="tt"></span>』</span>' +
      '<span class="tag"></span><span class="pos"></span></div>';
    el.querySelector(".before").textContent = r.before;
    el.querySelector(".hit").textContent = r.hit;
    el.querySelector(".after").textContent = r.after;
    el.querySelector(".work em").textContent = m[1] + " ";
    el.querySelector(".tt").textContent = m[0];
    const tag = el.querySelector(".tag");
    if (r.word !== state.query) tag.textContent = r.word;
    else tag.remove();
    el.querySelector(".pos").textContent = `${kanaNames[m[2]]} · ${m[3]} · ${fmt(r.pos)} バイト目`;
    kw.appendChild(el);
  }
  $("empty").hidden = rows.length > 0 || state.scanned === 0;
  if (state.scanned > 0 && rows.length === 0) {
    $("empty").hidden = false;
    $("empty").textContent = state.total === 0
      ? `「${state.query}」は走査した ${state.scanned} 枚には見つかりませんでした。`
      : "絞り込みの条件に合う用例がありません。";
  }

  // 作品別
  const pairs = [...state.byWork.entries()]
    .filter(([id]) => {
      const m = meta.works[id];
      if (!m) return false;
      if (state.facets.kana.size && !state.facets.kana.has(m[2])) return false;
      if (state.facets.genre.size && !state.facets.genre.has(m[3])) return false;
      return true;
    })
    .sort((a, b) => b[1] - a[1]).slice(0, 8);
  const max = pairs.length ? pairs[0][1] : 1;
  const dw = $("dist");
  dw.innerHTML = "";
  for (const [id, n] of pairs) {
    const m = meta.works[id];
    const row = document.createElement("div");
    row.className = "bar-row";
    row.innerHTML = '<span class="name"></span><span class="val"></span>' +
      '<span class="track"><span class="fill"></span></span>';
    row.querySelector(".name").textContent = m[0];
    row.querySelector(".name").title = `${m[1]}『${m[0]}』`;
    row.querySelector(".val").textContent = fmt(n);
    dw.appendChild(row);
    requestAnimationFrame(() => {
      row.querySelector(".fill").style.width = `${Math.round(n / max * 100)}%`;
    });
  }
}

function countBy(rows, pick) {
  const c = new Map();
  for (const r of rows) {
    const m = meta.works[r.id];
    if (!m) continue;
    const k = pick(m);
    c.set(k, (c.get(k) || 0) + 1);
  }
  return [...c.entries()].sort((a, b) => b[1] - a[1]);
}

function facet(elId, key, pairs, label) {
  const host = $(elId);
  host.innerHTML = "";
  const sel = state.facets[key];
  for (const [k, n] of pairs) {
    const b = document.createElement("button");
    b.type = "button";
    b.setAttribute("aria-pressed", sel.size === 0 || sel.has(k) ? "true" : "false");
    b.innerHTML = '<span class="nm"></span><span class="n"></span>';
    b.querySelector(".nm").textContent = label(k);
    b.querySelector(".n").textContent = fmt(n);
    b.addEventListener("click", () => {
      if (sel.has(k)) sel.delete(k); else sel.add(k);
      if (sel.size === pairs.length) sel.clear();
      render();
    });
    host.appendChild(b);
  }
}

// ---------------------------------------------------------------- 起動

async function boot() {
  const [wasmBytes, mf, mt] = await Promise.all([
    fetch("wasm/sakuin.wasm").then((r) => r.arrayBuffer()),
    fetch(BASE + "manifest.json").then((r) => r.json()),
    fetch(BASE + "works.json").then((r) => r.json()),
  ]);
  const { instance } = await WebAssembly.instantiate(wasmBytes, {});
  az = instance.exports;
  manifest = mf;
  meta = mt;
  kanaNames = mt.kana_names;

  if (az.az_format_version() !== mf.format_version) {
    $("index-state").textContent =
      `配信形式の版が食い違っています（索引 ${mf.format_version} / 照合器 ${az.az_format_version()}）`;
    return;
  }

  $("index-state").innerHTML =
    `索引 v<b>${az.az_format_version()}</b> ・ 収録 <b>${fmt(mf.works)}</b> 作 ・ ` +
    `本文 <b>${(mf.total_text / 1e6).toFixed(0)}</b> MB ・ シャード <b>${mf.shards.length}</b> 枚`;

  const about = $("about");
  about.innerHTML = "";
  const rows = [
    ["収録", `${fmt(mf.works)} 作`],
    ["本文", `${(mf.total_text / 1e6).toFixed(1)} MB`],
    ["索引", `${(mf.total_bytes / 1e6).toFixed(1)} MB`],
    ["シャード", `${mf.shards.length} 枚`],
    ["1 枚あたり", `${(Math.max(...mf.shards.map((s) => s.bytes)) / 1e6).toFixed(2)} MB`],
    [null, null],
    ["照合器", `${(wasmBytes.byteLength / 1024).toFixed(0)} KB (wasm)`],
    ["配信形式", `v${az.az_format_version()}`],
  ];
  for (const [k, v] of rows) {
    if (k === null) {
      const hr = document.createElement("div");
      hr.className = "rule";
      about.appendChild(hr);
      continue;
    }
    const dt = document.createElement("dt");
    dt.textContent = k;
    const dd = document.createElement("dd");
    dd.textContent = v;
    about.append(dt, dd);
  }

  $("note").innerHTML =
    "本文は配信していません。前後文脈は索引そのものから復元しています。" +
    "件数を数えるにはシャードを読む必要があるため、走査は 1 枚ずつ進みます — " +
    "途中で止めても、そこまでの用例はそのまま読めます。";

  $("run").addEventListener("click", search);
  $("q").addEventListener("keydown", (e) => { if (e.key === "Enter") search(); });
  for (const b of document.querySelectorAll(".chip")) {
    b.addEventListener("click", () => {
      b.setAttribute("aria-pressed", b.getAttribute("aria-pressed") === "true" ? "false" : "true");
    });
  }
  for (const b of document.querySelectorAll(".preset")) {
    b.addEventListener("click", () => { $("q").value = b.dataset.q; search(); });
  }
  $("q").focus();
}

boot().catch((e) => {
  $("index-state").textContent = `起動に失敗しました: ${e.message}`;
  console.error(e);
});
