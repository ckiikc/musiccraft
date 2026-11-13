#!/usr/bin/env python3
"""Check source files for common publication mistakes; print paths, never secret values."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
EXCLUDED = {".git", "target", "dist", "vendor", ".venv", "__pycache__", "test-output"}
# These exact synthetic tones and official Gradle wrapper are public test/build
# inputs. Hash pinning prevents personal media or a replacement binary slipping in.
PUBLIC_BINARIES = {
    "tests/fixtures/invalid-subframe.flac": "d1370ebf6a3984779ac38768459ea2ef72d53068f169b75d5ba9948002a995a7",
    'android/app/src/androidTest/assets/ratings/test.mp3': 'e4812fc3343cffa45e59eed158502975117dc27fcd3a9aabeba9990fc27a0359',
    'android/app/src/androidTest/assets/ratings/test.flac': '99e8cfdd8d460764ce629a4350976436600f5d6dcac239270d0d10d65590900a',
    "android/app/src/androidTest/assets/QA Alpha.flac": "95d1bc68f2348266c8e0cee4046d289a495eae4903dc403c74366b976b26fab6",
    "android/app/src/androidTest/assets/QA Beta.flac": "ecf635404f6c6b8a764fcfa0ab3c51bb6c4f8853d6733afb2166d8f1fae33fc9",
    "android/app/src/androidTest/assets/QA Gamma.flac": "6fc5804671be1f34cb8eca8616d4e1b779a5466c14d3942a0540d29e6246d461",
    "android/gradle/wrapper/gradle-wrapper.jar": "7d3a4ac4de1c32b59bc6a4eb8ecb8e612ccd0cf1ae1e99f66902da64df296172",
}
PRIVATE_NAMES = {"listening-stats.json", "playlists.json", "listening-v1.json", "rating-tag-journal.json", "rating-tags-v1.db", "state.ron", "ftp-sources.json", "tracks-seed.json", "tags.jsonl", "retries.json"}
MEDIA_SUFFIXES = {".dmg", ".wav", ".flac", ".mp3", ".m4a", ".audio", ".ron", ".log", ".key", ".p12", ".jks", ".keystore", ".apk"}
PATTERNS = {
    "home-directory": re.compile(r"/Users/[A-Za-z0-9_.-]+/|/home/[A-Za-z0-9_.-]+/"),
    "private-workspace": re.compile(r"/Volumes/Crucial_[A-Za-z0-9]+/"),
    "personal-network": re.compile(r"\b10[.]19[.]\d+[.]\d+\b"),
    "credential-in-url": re.compile(r"(?:ftps?|https?)://[^\s/\"']+:[^\s/@\"']+@"),
    "github-token": re.compile(r"\bgh[pousr]_[A-Za-z0-9]{30,}|\bgithub_pat_[A-Za-z0-9_]{40,}"),
    "private-key": re.compile(r"-----BEGIN (?:RSA |OPENSSH |EC )?PRIVATE KEY-----"),
}


def source_files():
    # A git archive is built from tracked files. Before git init, inspect the allowlisted tree.
    result = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "--show-toplevel"], capture_output=True, text=True)
    if result.returncode == 0 and Path(result.stdout.strip()).resolve() == ROOT:
        raw = subprocess.check_output(["git", "-C", str(ROOT), "ls-files", "--cached", "--others", "--exclude-standard", "-z"])
        files = sorted({ROOT / p.decode() for p in raw.split(b"\0") if p})
        if files:
            return files
    return [p for p in ROOT.rglob("*") if p.is_file() and not any(part in EXCLUDED for part in p.relative_to(ROOT).parts)]


def main():
    problems = []
    files = source_files()
    for path in files:
        relative = path.relative_to(ROOT)
        if path.is_symlink():
            problems.append([str(relative), "symlink"])
            continue
        if any(part in EXCLUDED for part in relative.parts) or path.name in PRIVATE_NAMES or path.name.startswith(".env"):
            problems.append([str(relative), "private-or-generated-file"])
        approved_hash = PUBLIC_BINARIES.get(relative.as_posix())
        if approved_hash and not path.is_symlink():
            if hashlib.sha256(path.read_bytes()).hexdigest() != approved_hash:
                problems.append([str(relative), "public-fixture-hash-mismatch"])
            continue
        if path.name == "local.properties":
            problems.append([str(relative), "local-sdk-configuration"])
        if path.suffix.lower() in MEDIA_SUFFIXES or path.stat().st_size > 8 * 1024 * 1024:
            problems.append([str(relative), "media-or-large-file"])
        data = path.read_bytes()
        if b"\0" in data:
            if path.suffix.lower() != ".png":
                problems.append([str(relative), "unexpected-binary"])
                continue
            # Also scan plaintext provenance/EXIF in the two generated PNG assets.
        text = data.decode("utf-8", errors="replace")
        for label, pattern in PATTERNS.items():
            for match in pattern.finditer(text):
                if label == "credential-in-url":
                    # These three negative parser fixtures deliberately exercise rejection.
                    # Only exact dummy credentials in a Rust test module are exempt.
                    test_start = text.find("#[cfg(test)]")
                    userinfo = match.group().split("://", 1)[1][:-1]
                    host = re.match(r"[A-Za-z0-9.-]+", text[match.end():])
                    if (test_start >= 0 and match.start() > test_start
                            and userinfo == "fixture:fixture-only" and host
                            and host.group() in {"nas.invalid", "phone.invalid", "localhost"}):
                        continue
                problems.append([str(relative), label])
                break
    print(json.dumps({"source_files": len(files), "findings": problems}, indent=2))
    return bool(problems)


if __name__ == "__main__":
    sys.exit(main())
