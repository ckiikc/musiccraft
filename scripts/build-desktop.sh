#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:?Usage: build-desktop.sh TARGET}"
# Remap local checkout/home locations out of distributed panic messages.
export CARGO_ENCODED_RUSTFLAGS="$(python3 - <<'PY'
import os
from pathlib import Path
flags=os.environ.get('CARGO_ENCODED_RUSTFLAGS','').split('\x1f') if os.environ.get('CARGO_ENCODED_RUSTFLAGS') else []
flags.extend([f'--remap-path-prefix={Path.home()}=/build/home',f'--remap-path-prefix={Path.cwd()}=/build/musiccraft'])
print('\x1f'.join(flags),end='')
PY
)"
cargo build --release --locked --target "$target" -j 2
