# -*- coding: utf-8 -*-
"""web/ を手元で配信する(表示確認用)。  python scripts/serve.py [ポート]"""
import functools, http.server, pathlib, socketserver, sys
ROOT = pathlib.Path(__file__).resolve().parent.parent / "web"
port = int(sys.argv[1]) if len(sys.argv) > 1 else 8787
Handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(ROOT))
Handler.extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                          ".wasm": "application/wasm"}
with socketserver.TCPServer(("127.0.0.1", port), Handler) as httpd:
    print(f"http://127.0.0.1:{port}/  ({ROOT})")
    httpd.serve_forever()
