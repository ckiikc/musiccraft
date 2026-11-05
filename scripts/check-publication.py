#!/usr/bin/env python3
"""Check source files for common publication mistakes; print paths, never secret values."""
from pathlib import Path
import json
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
EXCLUDED = {".git", "target", "dist", "vendor", ".venv", "__pycache__", "test-output"}
PRIVATE_NAMES = {"state.ron", "ftp-sources.json", "tracks-seed.json", "tags.jsonl", "retries.json"}
MEDIA_SUFFIXES = {".dmg", ".wav", ".flac", ".mp3", ".m4a", ".audio", ".ron", ".log", ".key", ".p12"}
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
