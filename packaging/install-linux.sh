#!/usr/bin/env sh
# Per-user install; music files and the data profile are never removed or rewritten.
set -eu
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
prefix=${MUSICCRAFT_PREFIX:-"$HOME/.local"}
mkdir -p "$prefix/lib/musiccraft" "$prefix/bin" "$prefix/share/applications" "$prefix/share/icons/hicolor/1024x1024/apps" "$prefix/share/mime/packages"
cp -R "$base/musiccraft" "$base/codecs" "$base/ThirdPartyLicenses" "$base/LICENSE" "$base/ATTRIBUTION.md" "$base/THIRD_PARTY.md" "$base/README.txt" "$prefix/lib/musiccraft/"
ln -sf "$prefix/lib/musiccraft/musiccraft" "$prefix/bin/musiccraft"
python3 - "$base/musiccraft.desktop" "$prefix/share/applications/io.github.ckiikc.musiccraft.desktop" "$prefix/lib/musiccraft/musiccraft" <<'PY'
import pathlib,sys
text=pathlib.Path(sys.argv[1]).read_text()
value=sys.argv[3].replace('\\','\\\\').replace('"','\\"').replace('`','\\`').replace('$','\\$').replace('%','%%')
pathlib.Path(sys.argv[2]).write_text(text.replace('Exec=musiccraft %F',f'Exec="{value}" %F'))
PY
cp "$base/musiccraft.png" "$prefix/share/icons/hicolor/1024x1024/apps/io.github.ckiikc.musiccraft.png"
cp "$base/musiccraft.xml" "$prefix/share/mime/packages/io.github.ckiikc.musiccraft.xml"
if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database "$prefix/share/applications"; fi
if command -v update-mime-database >/dev/null 2>&1; then update-mime-database "$prefix/share/mime"; fi
printf 'MusicCraft installed in %s\nLibrary and default file handlers preserved.\n' "$prefix/lib/musiccraft"
