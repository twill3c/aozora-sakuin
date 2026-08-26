# -*- coding: utf-8 -*-
"""品質ゲートを一括で回す(SPEC §4)。

  python scripts/verify.py [--fast]

段:
  1. Python  — 選定ゲート / 正規化ゲート
  2. Rust    — fmt / clippy / test(オラクル)
  3. wasm    — wasm32 ビルドと配置
  4. 実地    — Node で wasm 越しの結果を本文と突き合わせる

--fast は 3〜4 を飛ばす(索引が未構築のときも通る)。
"""
import pathlib
import shutil
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUST = ROOT / "rust"
WASM_SRC = RUST / "target/wasm32-unknown-unknown/release/aozora_sakuin.wasm"
WASM_DST = ROOT / "web/wasm/sakuin.wasm"


def run(label, cmd, cwd=ROOT, optional=False):
    t = time.time()
    print(f"\n=== {label} ===", flush=True)
    r = subprocess.run(cmd, cwd=cwd, shell=isinstance(cmd, str))
    dt = time.time() - t
    if r.returncode != 0:
        if optional:
            print(f"--- {label}: 省略(実行環境に無い) {dt:.1f}s")
            return True
        print(f"--- {label}: 不合格 ({dt:.1f}s)")
        return False
    print(f"--- {label}: 合格 ({dt:.1f}s)")
    return True


def run_with_server(label, cmd, port=8799):
    """web/ を一時的に配信してから検査を回す。静的配信のまま動くことの裏を取る。"""
    srv = subprocess.Popen(
        [sys.executable, str(ROOT / "scripts/serve.py"), str(port)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, cwd=ROOT)
    try:
        for _ in range(50):
            try:
                import urllib.request
                urllib.request.urlopen(f"http://127.0.0.1:{port}/index.html", timeout=1)
                break
            except Exception:  # noqa: BLE001
                time.sleep(0.1)
        return run(label, cmd + [f"http://127.0.0.1:{port}/"])
    finally:
        srv.terminate()


def main():
    fast = "--fast" in sys.argv
    ok = True

    ok &= run("Python ゲート", [sys.executable, "-m", "pytest", "tests/", "-q"])
    ok &= run("Rust 整形", ["cargo", "fmt", "--check"], cwd=RUST)
    ok &= run("Rust 静的検査", ["cargo", "clippy", "--all-targets", "--", "-D", "warnings"], cwd=RUST)
    ok &= run("Rust オラクル", ["cargo", "test", "--release"], cwd=RUST)

    if not fast:
        ok &= run("wasm ビルド",
                  ["cargo", "build", "--release", "--target", "wasm32-unknown-unknown", "--lib"],
                  cwd=RUST)
        if WASM_SRC.exists():
            WASM_DST.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(WASM_SRC, WASM_DST)
            print(f"    {WASM_DST.relative_to(ROOT)} ({WASM_DST.stat().st_size:,} バイト)")
        if (ROOT / "web/index").exists():
            ok &= run("wasm 実地検査", ["node", "scripts/verify_wasm.mjs"])
            ok &= run_with_server("画面の実地検査", ["node", "scripts/verify_ui.mjs"])
        else:
            print("\n=== wasm 実地検査 ===\n--- 省略: web/index が無い(先に build_shards)")

    print("\n" + ("すべて合格" if ok else "不合格あり"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
