#!/usr/bin/env python3
"""Run isolated FTP integration tests. Requires pyftpdlib and ffmpeg, no personal files."""
import logging, math, os, pathlib, struct, subprocess, tempfile, threading, wave, zlib
from pyftpdlib.authorizers import DummyAuthorizer
from pyftpdlib.handlers import FTPHandler
from pyftpdlib.servers import FTPServer
logging.disable(logging.CRITICAL)
repo = pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="aurea-ftp-") as temporary:
    root = pathlib.Path(temporary)
    album = root / "Album Città"
    album.mkdir()
    wav = album / "01 Opening.wav"
    with wave.open(str(wav), "wb") as file:
        file.setparams((2, 2, 44100, 0, "NONE", "not compressed"))
        samples = [int(1600 * math.sin(2 * math.pi * 440 * i / 44100)) for i in range(44100 * 4)]
        file.writeframes(b"".join(struct.pack("<hh", s, s // 2) for s in samples))
    for filename, codec in [("02 Interlude.flac", "flac"), ("03 Finale.m4a", "alac"), ("04 Encore.mp3", "libmp3lame")]:
        subprocess.run(["ffmpeg", "-v", "error", "-i", str(wav), "-c:a", codec, "-metadata", "title=" + filename[3:].rsplit(".", 1)[0], "-metadata", "artist=Aurea Test Ensemble", "-metadata", "album=Network Sessions", "-metadata", "replaygain_track_gain=-6.0 dB", str(album / filename)], check=True)
    (root / "    Leading.flac").write_bytes((album / "02 Interlude.flac").read_bytes())
    def syncsafe(n):
        return bytes((n >> shift) & 127 for shift in (21,14,7,0))
    picture = b"APIC" + syncsafe(2*1024*1024) + bytes(2) + bytes(2*1024*1024)
    text = b"\x03After Picture"
    body = picture + b"TIT2" + syncsafe(len(text)) + bytes(2) + text + bytes(1024*1024)
    (root / "Large Picture.mp3").write_bytes(b"ID3\x04\x00\x00" + syncsafe(len(body)) + body + bytes(256))
    # A large artwork block before Vorbis comments, then synthetic payload: tag reads must seek
    # past both instead of transferring this 6 MiB file. It is never decoded or played.
    streaminfo = (album / "02 Interlude.flac").read_bytes()[8:42]
    comment = struct.pack("<I", 7) + b"fixture"
    fields = [b"TITLE=Range Title", b"ARTIST=Aurea Test Ensemble", b"ALBUM=Network Sessions"]
    comment += struct.pack("<I", len(fields)) + b"".join(struct.pack("<I",len(v)) + v for v in fields)
    with (root / ".range-probe.bin").open("wb") as probe:
        probe.write(b"fLaC" + bytes([0,0,0,34]) + streaminfo)
        probe.write(bytes([6]) + (2*1024*1024).to_bytes(3,"big"))
        probe.write(bytes(2*1024*1024))
        probe.write(bytes([0x84]) + len(comment).to_bytes(3,"big") + comment)
        probe.write(bytes(4*1024*1024))
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
    raw = b"".join(b"\0" + bytes([50, 75, 95]) * 64 for _ in range(64))
    (album / "cover.png").write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 64, 64, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))
    authorizer = DummyAuthorizer()
    authorizer.add_user("fixture", "fixture-only", str(root), perm="elr")
    authorizer.add_user("fixture-write", "fixture-only", str(root), perm="elradfmwMT")
    metrics = root / ".sync-metrics"
    def record(event):
        with metrics.open("a") as file:
            file.write(event + "\n")
    short_audio_range = [False]
    slow_audio_range = [False]
    class Handler(FTPHandler):
        def ftp_RETR(self, file):
            # Delay a range reached after the startup reserve is ready.
            if (not slow_audio_range[0] and pathlib.Path(file).name == "Hour [gapless].wav"
                    and self._restart_position == 3 * 1024 * 1024):
                slow_audio_range[0] = True
                threading.Event().wait(8)
            # Truncate exactly one late range of the progressive playback test.
            if (not short_audio_range[0] and pathlib.Path(file).name == "Hour [gapless].wav"
                    and self._restart_position > 500_000_000):
                short_audio_range[0] = True
                self._restart_position = 0
                self.push_dtp_data(b"\0", cmd="RETR")
                return
            return super().ftp_RETR(file)

        def pre_process_command(self, line, cmd, arg):
            if cmd == "SIZE" and "Leading.flac" in arg and not arg.startswith("/"):
                self.respond("550 Absolute path required for this filename")
                return
            super().pre_process_command(line, cmd, arg)

        def on_login(self, username):
            record("login")
        def on_file_sent(self, filename):
            record("download")
    Handler.authorizer = authorizer
    server = FTPServer(("127.0.0.1", 0), Handler)
    port = server.socket.getsockname()[1]
    class LegacyHandler(Handler):
        def ftp_MLSD(self, path):
            self.respond("502 MLSD unsupported")
    legacy = FTPServer(("127.0.0.1", 0), LegacyHandler)
    legacy_port = legacy.socket.getsockname()[1]
    stopped = threading.Event()
    def serve():
        while not stopped.is_set():
            server.serve_forever(timeout=0.1, blocking=False, handle_exit=False)
            legacy.serve_forever(timeout=0.1, blocking=False, handle_exit=False)
    thread = threading.Thread(target=serve, daemon=True)
    thread.start()
    env = os.environ.copy()
    env.update(AUREA_TEST_FTP_URL=f"ftp://127.0.0.1:{port}/", AUREA_TEST_FTP_ROOT=str(root), AUREA_TEST_FTP_LEGACY_URL=f"ftp://127.0.0.1:{legacy_port}/")
    try:
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_metadata_edges", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        (root / "    Leading.flac").unlink()
        (root / "Large Picture.mp3").unlink()
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_tag_priority", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_library_tags", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_album_tags", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_scan", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_incremental", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_rediscovery", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        (root / "Upload Album").mkdir()
        (root / "Read Only Album").mkdir()
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_cover_save", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        nested = root / "Artwork Only" / "Scans"
        nested.mkdir(parents=True)
        (nested / "Front #1.jpg").write_bytes((album / "cover.png").read_bytes())
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_nested_cover", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        for name, filename in [
            ("Literal [2003 Remaster]", "01 Track [Live].wav"),
            ("Literal [Disc 1]", "02 Track.wav"),
            ("Literal {Mono,Stereo}", "03 Track {one,two}.wav"),
        ]:
            folder = root / name
            folder.mkdir()
            (folder / filename).write_bytes(wav.read_bytes())
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_literal_brackets", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        subprocess.run(["cargo", "test", "-j", "2", "--locked", "--offline", "live_ftp_progressive_hour", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        assert short_audio_range[0], "truncated range fixture did not run"
        assert slow_audio_range[0], "slow range fixture did not run"
    finally:
        stopped.set()
        thread.join(timeout=2)
        server.close_all()
        legacy.close_all()
