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

const state = {
  query: "",
  forms: [],               // [{form, ratio, base}]
  counts: new Map(),       // form -> 件数
  rows: [],                // 用例
  byWork: new Map(),       // 作品ID -> 件数
  scanned: 0,
  bytes: 0,
  total: 0,
  running: false,
  abort: false,
  next: 0,                 // 次に走査するシャード
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

async function scanFrom(start) {
  state.running = true;
  state.abort = false;
  $("stop").hidden = false;
  $("more").hidden = true;
  $("run").disabled = true;

  for (let k = start; k < manifest.shards.length; k++) {
    if (state.abort) { state.next = k; break; }
    const s = manifest.shards[k];
    let buf;
    try {
      buf = new Uint8Array(await (await fetch(BASE + s.file)).arrayBuffer());
    } catch (e) {
      console.warn(`${s.file} を取得できない`, e);
      continue;
    }
    state.bytes += buf.length;
    const p = put(buf);
    const h = az.az_shard_load(p, buf.length);
    if (h < 0) { az.az_free(p, buf.length); continue; }

    for (const f of state.forms) {
      const n = countIn(h, f.form);
      if (n > 0) {
        state.counts.set(f.form, (state.counts.get(f.form) || 0) + n);
        state.total += n;
        if (state.rows.length < SHOW_MAX) {
          for (const r of kwicIn(h, f.form, PER_SHARD)) {
            state.rows.push(r);
            state.byWork.set(r.id, (state.byWork.get(r.id) || 0) + 1);
          }
        }
      }
    }
    az.az_shard_drop(h);
    az.az_free(p, buf.length);
    state.scanned = k + 1;
    state.next = k + 1;
    render();
    await new Promise((r) => setTimeout(r, 0));  // 画面を描かせる
  }

  state.running = false;
  $("run").disabled = false;
  $("stop").hidden = true;
  $("more").hidden = state.next >= manifest.shards.length;
  render();
}

async function search() {
  const q = $("q").value.trim();
  if (!q || !az) return;
  const useVariants = $("opt-variants").getAttribute("aria-pressed") === "true";
  const risky = $("opt-risky").getAttribute("aria-pressed") === "true";

  state.query = q;
  state.forms = [{ form: q, ratio: 100, base: true }];
  if (useVariants) state.forms.push(...variantsOf(q, risky));
  state.counts = new Map();
  state.rows = [];
  state.byWork = new Map();
  state.scanned = 0;
  state.bytes = 0;
  state.total = 0;
  state.next = 0;
  state.facets = { kana: new Set(), genre: new Set() };

  $("readout").hidden = false;
  $("empty").hidden = true;
  await scanFrom(0);
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
  $("stop").addEventListener("click", () => { state.abort = true; });
  $("more").addEventListener("click", () => scanFrom(state.next));
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
