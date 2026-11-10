#!/usr/bin/env python3
"""macOS-only synthetic HTTPS/Bonjour late-sharing regression; no personal profile."""
import base64
import hashlib
import http.server
import json
import os
from pathlib import Path
import secrets
import ssl
import subprocess
import tempfile
import threading
import time
import uuid
from urllib.parse import urlencode


def run(*args, **kwargs):
    return subprocess.check_output(args, **kwargs)


def main():
    repo = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="musiccraft-reconnect-") as temp:
        folder = Path(temp)
        identity = str(uuid.uuid4())
        token = secrets.token_urlsafe(32)
        key, cert = folder / "key.pem", folder / "cert.pem"
        run("openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1",
            "-nodes", "-days", "1", "-subj", "/CN=MusicCraft Synthetic QA", "-keyout", str(key),
            "-out", str(cert), stderr=subprocess.DEVNULL)
        public = run("openssl", "pkey", "-in", str(key), "-pubout", "-outform", "DER")
        pin = base64.b64encode(hashlib.sha256(public).digest()).decode()
        # The saved source is deliberately offline; discovery must recover its new endpoint.
        (folder / "pairing.txt").write_text(
            "musiccraft://pair?" + urlencode(dict(v=1, host="127.0.0.1", port=9,
                                                id=identity, pin=pin, token=token)))
        expected = "Basic " + base64.b64encode(f"musiccraft:{token}".encode()).decode()
        calls = {"status": 0, "catalog": 0}

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                if self.headers.get("Authorization") != expected:
                    self.send_error(401)
                    return
                if self.path == "/v1/status":
                    calls["status"] += 1
                    value = {"protocol": 1, "device_id": identity, "sharing": True}
                elif self.path == "/v1/catalog":
                    calls["catalog"] += 1
                    value = {"protocol": 1, "device_id": identity, "tracks": [{
                        "id": "42", "filename": "42.flac", "title": "Synthetic reconnect",
                        "artist": "MusicCraft QA", "album": "Reconnect QA", "duration": 2.0,
                        "number": 1, "disc": 1, "bytes": 10}]}
                else:
                    self.send_error(404)
                    return
                body = json.dumps(value).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        env = dict(os.environ, MUSICCRAFT_QA_RECONNECT=str(folder))
        server = publisher = None
        enabled_at = None
        enabled_epoch = None
        with (folder / "test.log").open("w+") as log:
            child = subprocess.Popen([
                "cargo", "test", "--locked", "-j", "2",
                "live_paired_phone_returns_after_sharing_is_enabled_without_restart",
                "--", "--ignored", "--nocapture"], cwd=repo, env=env, stdout=log, stderr=log)
            deadline = time.monotonic() + 120
            try:
                while child.poll() is None and time.monotonic() < deadline:
                    if enabled_at is None and (folder / "offline-observed").exists():
                        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
                        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                        context.load_cert_chain(cert, key)
                        server.socket = context.wrap_socket(server.socket, server_side=True)
                        threading.Thread(target=server.serve_forever, daemon=True).start()
                        hostname = f"musiccraft-qa-{identity[:8]}.local."
                        publisher = subprocess.Popen([
                            "/usr/bin/dns-sd", "-P", f"MusicCraft-{identity[:8]}",
                            "_musiccraft._tcp", "local.", str(server.server_port), hostname,
                            "127.0.0.1", "protocol=1", f"id={identity}"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                        enabled_at = time.monotonic()
                        enabled_epoch = time.time()
                    if (folder / "recovered.json").exists():
                        break
                    time.sleep(0.05)
                child.wait(timeout=10)
                log.seek(0)
                print(log.read())
                if child.returncode != 0 or enabled_at is None:
                    raise RuntimeError("late-sharing regression failed")
                result = json.loads((folder / "recovered.json").read_text())
                recovered_after = (folder / "recovered.json").stat().st_mtime - enabled_epoch
                result.update(seconds_after_sharing_enabled=round(recovered_after, 3), authenticated_requests=calls)
                print(json.dumps(result, indent=2))
            finally:
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=10)
                if publisher:
                    publisher.terminate()
                    publisher.wait(timeout=5)
                if server:
                    server.shutdown()
                    server.server_close()


if __name__ == "__main__":
    main()
