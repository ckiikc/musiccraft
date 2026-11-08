#!/bin/zsh
# Experimental local package. No installation or GitHub release is performed.
set -eu
cd "$(dirname "$0")/.."
profile="${1:-release}"
lms_mode="${2:-}"
if [[ "$profile" != release && "$profile" != debug ]]; then
  print -u2 'Usage: scripts/package-macos.sh [release|debug] [--with-lms-local]'
  exit 2
fi
if [[ -n "$lms_mode" && "$lms_mode" != --with-lms-local ]]; then
  print -u2 'Only --with-lms-local is supported as a second argument.'
  exit 2
fi
# Keep builder home directories and checkout paths out of distributed panic
# locations. Encoded flags preserve spaces in a checkout path correctly.
export CARGO_ENCODED_RUSTFLAGS="$(python3 - <<'PYTHON'
import os
from pathlib import Path
existing = os.environ.get('CARGO_ENCODED_RUSTFLAGS')
flags = existing.split('\x1f') if existing else os.environ.get('RUSTFLAGS', '').split()
flags.extend([f'--remap-path-prefix={Path.home()}=/build/home',
              f'--remap-path-prefix={Path.cwd()}=/build/musiccraft'])
print('\x1f'.join(flags), end='')
PYTHON
)"
if [[ "$profile" == release ]]; then
  cargo build --release --locked -j 2
else
  cargo build --locked -j 2
fi
target_dir="${CARGO_TARGET_DIR:-$PWD/target}"
bundle="$PWD/dist/MusicCraft.app"
# Replace this generated artifact instead of merging a previous vendor payload.
python3 - "$bundle" <<'PYTHON'
import pathlib, shutil, sys
p = pathlib.Path(sys.argv[1])
if p.is_symlink():
    raise SystemExit('Refusing to replace a symlink at the generated bundle path')
if p.exists():
    shutil.rmtree(p)
PYTHON
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
if [[ "$lms_mode" == --with-lms-local ]]; then
  print -u2 'Local integrated-LMS experiment only: third-party redistribution is not cleared.'
  python3 scripts/prepare-lms-runtime.py >/dev/null
  ditto vendor/LyrionRuntime.app "$bundle/Contents/Resources/LyrionRuntime.app"
  codesign --force --deep --sign - --identifier io.github.ckiikc.musiccraft.server "$bundle/Contents/Resources/LyrionRuntime.app"
  cp vendor/lms-runtime.json "$bundle/Contents/Resources/lms-runtime.json"
fi
cp LICENSE THIRD_PARTY.md ATTRIBUTION.md "$bundle/Contents/Resources/"
if [[ -f docs/DEPENDENCIES.tsv ]]; then
  cp docs/DEPENDENCIES.tsv "$bundle/Contents/Resources/"
fi
python3 scripts/collect-licenses.py "$bundle/Contents/Resources/ThirdPartyLicenses"
cp "$target_dir/$profile/musiccraft" "$bundle/Contents/MacOS/MusicCraft"
python3 - "$bundle" <<'PYTHON'
import pathlib, plistlib, sys, tomllib
version = tomllib.loads(pathlib.Path('Cargo.toml').read_text())['package']['version']
bundle = pathlib.Path(sys.argv[1])
info = {'CFBundleName': 'MusicCraft', 'CFBundleDisplayName': 'MusicCraft',
        'CFBundleIdentifier': 'io.github.ckiikc.musiccraft', 'CFBundleExecutable': 'MusicCraft',
        'CFBundleVersion': version, 'CFBundleShortVersionString': version,
        'CFBundlePackageType': 'APPL', 'LSMinimumSystemVersion': '12.0',
        'NSHighResolutionCapable': True, 'NSHumanReadableCopyright': 'Copyright 2026 MusicCraft contributors. MIT.',
        'CFBundleIconFile': 'MusicCraft',
        'NSLocalNetworkUsageDescription': 'Find paired MusicCraft phones and stream music on your local network.',
        'NSBonjourServices': ['_musiccraft._tcp.']}
(bundle / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
PYTHON
mkdir -p dist/MusicCraft.iconset
swift -module-cache-path "${TMPDIR:-/tmp}/musiccraft-swift-cache" scripts/icon.swift "$PWD/assets/app-icon/musiccraft-1024.png" "$PWD/dist/MusicCraft.iconset/icon_512x512@2x.png"
sips -z 1024 1024 dist/MusicCraft.iconset/icon_512x512@2x.png --out dist/icon-master.png >/dev/null
for icon_size in 16 32 128 256 512; do
  sips -z "$icon_size" "$icon_size" dist/icon-master.png --out "dist/MusicCraft.iconset/icon_${icon_size}x${icon_size}.png" >/dev/null
  double_size=$((icon_size * 2))
  sips -z "$double_size" "$double_size" dist/icon-master.png --out "dist/MusicCraft.iconset/icon_${icon_size}x${icon_size}@2x.png" >/dev/null
done
iconutil -c icns dist/MusicCraft.iconset -o "$bundle/Contents/Resources/MusicCraft.icns"
python3 scripts/document-types.py "$bundle"
python3 scripts/app-uuid.py "$bundle/Contents/MacOS/MusicCraft" io.github.ckiikc.musiccraft
codesign --force --sign - --identifier io.github.ckiikc.musiccraft "$bundle"
codesign --verify --deep --strict "$bundle"
print "$bundle"
