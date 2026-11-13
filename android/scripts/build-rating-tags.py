#!/usr/bin/env python3
"""Build the same audited metadata engine for Android; requires Rust targets and NDK."""
import os, pathlib, platform, shutil, subprocess
root = pathlib.Path(__file__).resolve().parents[1]
sdk = pathlib.Path(os.environ.get('ANDROID_HOME', pathlib.Path.home() / 'Library/Android/sdk'))
ndk = sorted((sdk / 'ndk').iterdir())[-1]
host = 'darwin-x86_64' if platform.system() == 'Darwin' else 'linux-x86_64'
for target, abi, compiler in [('aarch64-linux-android', 'arm64-v8a', 'aarch64-linux-android26-clang'), ('x86_64-linux-android', 'x86_64', 'x86_64-linux-android26-clang')]:
    env = os.environ.copy()
    # Distributed native panic locations must not identify the build machine.
    remaps = [f"--remap-path-prefix={pathlib.Path.home()}=/build/home", f"--remap-path-prefix={root.parent}=/build/musiccraft"]
    previous = env.get('CARGO_ENCODED_RUSTFLAGS', '')
    env['CARGO_ENCODED_RUSTFLAGS'] = '\x1f'.join(([previous] if previous else []) + remaps)
    env['CARGO_TARGET_' + target.upper().replace('-', '_') + '_LINKER'] = str(ndk / 'toolchains/llvm/prebuilt' / host / 'bin' / compiler)
    subprocess.run(['cargo', 'build', '--locked', '--release', '--target', target, '-j', '2', '--manifest-path', str(root.parent / 'crates/musiccraft-tags/Cargo.toml')], env=env, check=True)
    dest = root / 'app/src/main/jniLibs' / abi
    dest.mkdir(parents=True, exist_ok=True)
    shutil.copy2(root.parent / 'crates/musiccraft-tags/target' / target / 'release/libmusiccraft_tags.so', dest)
subprocess.run(['python3', str(root / 'scripts/generate-native-notices.py')], check=True)
