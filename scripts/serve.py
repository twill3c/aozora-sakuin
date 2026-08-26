# -*- coding: utf-8 -*-
"""web/ を手元で配信する。  python scripts/serve.py [ポート]

**Range に対応している**。索引は丸ごと落とさず必要なバイトだけ読む設計なので、
Range を返さない配信では本番と違う経路を検査してしまう。標準の
SimpleHTTPRequestHandler は Range を無視して全体を返すため、ここで足している。
"""
import functools
import http.server
import pathlib
import re
import socketserver
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent / "web"
RANGE = re.compile(r"bytes=(\d*)-(\d*)$")


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".wasm": "application/wasm",
        ".azsk": "application/octet-stream",
    }

    def send_head(self):
        rng = self.headers.get("Range")
        if not rng:
            resp = super().send_head()
            if resp:
                pass
            return resp

        path = self.translate_path(self.path)
        p = pathlib.Path(path)
        if not p.is_file():
            return super().send_head()

        m = RANGE.match(rng.strip())
        if not m:
            self.send_error(416, "Range を解釈できない")
            return None
        size = p.stat().st_size
        start_s, end_s = m.group(1), m.group(2)
        if start_s == "":                      # bytes=-N (末尾から N バイト)
            length = int(end_s or 0)
            start = max(0, size - length)
            end = size - 1
        else:
            start = int(start_s)
            end = int(end_s) if end_s else size - 1
        if start >= size or start > end:
            self.send_response(416)
            self.send_header("Content-Range", f"bytes */{size}")
            self.end_headers()
            return None
        end = min(end, size - 1)

        f = open(path, "rb")
        f.seek(start)
        self.send_response(206)
        self.send_header("Content-Type", self.guess_type(path))
        self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.send_header("Accept-Ranges", "bytes")
        self.end_headers()
        # copyfile が最後まで送ってしまわないよう、必要な分だけ読んで返す
        data = f.read(end - start + 1)
        f.close()
        self.wfile.write(data)
        return None

    def end_headers(self):
        self.send_header("Accept-Ranges", "bytes")
        super().end_headers()

    def log_message(self, *args):
        pass


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True
    # 索引は多数のシャードへ同時に問い合わせるので、待ち行列を深く取る。
    # 既定の 5 では並列接続が拒否される
    request_queue_size = 512


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8787
    handler = functools.partial(Handler, directory=str(ROOT))
    with Server(("127.0.0.1", port), handler) as httpd:
        print(f"http://127.0.0.1:{port}/  ({ROOT})  Range 対応")
        httpd.serve_forever()


if __name__ == "__main__":
    main()
