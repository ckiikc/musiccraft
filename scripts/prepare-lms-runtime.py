#!/usr/bin/env python3
"""Prepare the pinned official macOS runtime for a local MusicCraft bundle.

No installer is executed; the vendor cache and generated bundle are ignored.
The upstream component remains unmodified, including its license notices.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
URL = "https://downloads.lms-community.org/LyrionMusicServer_v9.1.1/LyrionMusicServer-9.1.1.dmg"
SHA256 = "2ae51698eb26f1e68fb15b03c994bf8e14b9bb7d9eb8c73d30526aced1494d80"
VENDOR = ROOT / "vendor"
TARGET = VENDOR / "LyrionRuntime.app"
MANIFEST = VENDOR / "lms-runtime.json"
expected = {"version": "9.1.1", "url": URL, "sha256": SHA256}
if MANIFEST.is_file() and json.loads(MANIFEST.read_text()) == expected and (TARGET / "Contents/MacOS/perl").is_file():
    print(TARGET)
    raise SystemExit(0)
VENDOR.mkdir(exist_ok=True)
dmg = Path(os.environ.get("MUSICCRAFT_LMS_DMG", VENDOR / "LyrionMusicServer-9.1.1.dmg"))
if not dmg.is_file():
    temporary = VENDOR / "download.part"
    with urllib.request.urlopen(URL, timeout=45) as source, temporary.open("wb") as output:
        shutil.copyfileobj(source, output)
    temporary.replace(dmg)
if hashlib.file_digest(dmg.open("rb"), "sha256").hexdigest() != SHA256:
    raise SystemExit("Official LMS runtime checksum mismatch")
with tempfile.TemporaryDirectory(prefix="musiccraft-runtime-") as directory:
    mount = Path(directory) / "volume"
    subprocess.run(["/usr/bin/hdiutil", "attach", str(dmg), "-readonly", "-nobrowse", "-mountpoint", str(mount)], check=True, stdout=subprocess.DEVNULL)
    try:
        source = mount / "Lyrion Music Server.app/Contents/MacOS/Lyrion Music Server.app"
        staging = VENDOR / "LyrionRuntime.staging.app"
        if staging.exists():
            shutil.rmtree(staging)
        shutil.copytree(source, staging, symlinks=True)
        if TARGET.exists():
            shutil.rmtree(TARGET)
        staging.replace(TARGET)
        MANIFEST.write_text(json.dumps(expected, indent=2))
    finally:
        subprocess.run(["/usr/bin/hdiutil", "detach", str(mount)], check=True, stdout=subprocess.DEVNULL)
print(TARGET)
