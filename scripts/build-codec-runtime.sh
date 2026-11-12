#!/usr/bin/env bash
# Build an unmodified, minimal LGPL FFmpeg CLI. No GPL/nonfree/external codec libraries.
set -euo pipefail
cd "$(dirname "$0")/.."
platform="${1:-linux}"
case "$platform" in linux|windows|macos) ;; *) echo 'Usage: build-codec-runtime.sh linux|windows|macos' >&2; exit 2;; esac
version=8.0.1
archive="vendor/codec-source/ffmpeg-$version.tar.xz"
mkdir -p vendor/codec-source
if [[ ! -f "$archive" ]]; then
  curl --fail --location --proto '=https' --tlsv1.2 "https://ffmpeg.org/releases/ffmpeg-$version.tar.xz" -o "$archive"
fi
python3 - "$archive" <<'PY'
import hashlib,sys
from pathlib import Path
assert hashlib.sha256(Path(sys.argv[1]).read_bytes()).hexdigest() == '05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41', 'FFmpeg source hash mismatch'
PY
source_dir="$PWD/vendor/codec-source/ffmpeg-$version"
if [[ ! -d "$source_dir" ]]; then tar -xf "$archive" -C vendor/codec-source; fi
build="$PWD/vendor/codec-build-$platform"
out="$PWD/vendor/codecs-$platform"
mkdir -p "$build" "$out"
root="$PWD"
cd "$build"
flags=(--prefix="$out" --disable-everything --disable-autodetect --disable-network
  --disable-gpl --disable-nonfree --disable-version3 --disable-doc --disable-debug
  --disable-x86asm --disable-avdevice --disable-swscale
  --enable-ffmpeg --enable-ffprobe --enable-avfilter --enable-swresample
  --enable-protocol=file,pipe --enable-demuxer=dsf,iff,ape,asf,wv,ogg,wav
  --enable-decoder=dsd_lsbf,dsd_msbf,dsd_lsbf_planar,dsd_msbf_planar,ape,wmav1,wmav2,wmapro,wmalossless,wmavoice,dst,wavpack,opus,pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le
  --enable-parser=opus --enable-encoder=pcm_s32le,pcm_f32le --enable-muxer=pcm_s32le,pcm_f32le
  --enable-filter=anull,aformat,aresample,abuffer,abuffersink,atrim,asetpts
  --extra-cflags="-ffile-prefix-map=$root=/build/musiccraft")
if [[ "$platform" == windows ]]; then
  flags+=(--target-os=mingw32 --arch=x86_64 --cross-prefix=x86_64-w64-mingw32- --enable-cross-compile --extra-ldflags=-static-libgcc)
fi
"$source_dir/configure" "${flags[@]}" > configure.log
if grep -q "did not match" configure.log; then cat configure.log >&2; exit 1; fi
make -j 2 > build.log 2>&1
suffix=''; [[ "$platform" == windows ]] && suffix='.exe'
cp "ffmpeg$suffix" "ffprobe$suffix" "$out/"
mkdir -p "$out/licenses"
cp "$source_dir/COPYING.LGPLv2.1" "$out/licenses/"
if [[ "$platform" == windows ]]; then
  for package in mingw-w64-common gcc-mingw-w64-base; do
    test -f "/usr/share/doc/$package/copyright"
    cp "/usr/share/doc/$package/copyright" "$out/licenses/$package-copyright.txt"
  done
fi
cp "$root/$archive" "$out/licenses/"
cp "$root/scripts/build-codec-runtime.sh" "$out/licenses/"
printf '%s\n' 'FFmpeg 8.0.1; unmodified LGPL-2.1-or-later sources and exact build recipe included.' 'https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz' 'SHA-256: 05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41' > "$out/licenses/README.txt"
echo "$out"
