#!/usr/bin/env python3
"""Bundle license texts from the exact resolved native dependencies; never emit build paths."""
import json
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
manifest = root.parent / 'crates/musiccraft-tags/Cargo.toml'
metadata = json.loads(subprocess.check_output([
    'cargo', 'metadata', '--locked', '--format-version', '1',
    '--manifest-path', str(manifest), '--filter-platform', 'aarch64-linux-android',
]))
parts = ['MusicCraft metadata and synchronization engines: MIT.\nNative dependency notices from Cargo.lock-pinned sources.\n']
for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
    if not package.get('source'):
        continue
    directory = pathlib.Path(package['manifest_path']).parent
    files = [p for p in directory.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE', 'COPYING', 'COPYRIGHT', 'NOTICE'))]
    for path in sorted(files):
        if path.stat().st_size > 1024 * 1024:
            raise RuntimeError('Unexpected license size')
        parts.append(f"\n===== {package['name']} {package['version']} / {path.name} =====\n\n" + path.read_text())
out = root / 'app/src/main/assets/native-licenses.txt'
out.write_text(''.join(parts))
