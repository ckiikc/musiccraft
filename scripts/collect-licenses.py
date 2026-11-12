#!/usr/bin/env python3
"""Collect pinned dependency notices without writing local paths into the bundle."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def main():
    if len(sys.argv) not in (2,4) or (len(sys.argv)==4 and sys.argv[2]!="--target"):
        raise SystemExit("Usage: scripts/collect-licenses.py OUTPUT_DIRECTORY [--target TRIPLE]")
    output = Path(sys.argv[1]).resolve()
    generated = (ROOT / "dist").resolve()
    if not output.is_relative_to(generated):
        raise SystemExit("License output must be inside the generated dist directory")
    host = next(line.split(": ", 1)[1] for line in subprocess.check_output(
        ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    if len(sys.argv)==4:
        host=sys.argv[3]
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--offline", "--format-version", "1",
        "--filter-platform", host,
    ], cwd=ROOT, text=True))
    resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
    packages = [p for p in metadata["packages"] if p["id"] in resolved and p["source"]]
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    checksums = {(p["name"], p["version"]): p.get("checksum", "") for p in lock["package"]}
    output.mkdir(parents=True, exist_ok=True)
    shutil.copytree(ROOT / "docs/licenses", output / "upstream", dirs_exist_ok=True)
    manifest = json.loads((ROOT / "docs/licenses/manifest.json").read_text())
    supplemental = {package for row in manifest for package in row["packages"]}
    index = ["name\tversion\tdeclared_license\tcrate_source\tcrate_sha256"]
    for package in sorted(packages, key=lambda p: (p["name"], p["version"])):
        name, version = package["name"], package["version"]
        source = Path(package["manifest_path"]).parent.resolve()
        destination = output / f"{name}-{version}"
        destination.mkdir(exist_ok=True)
        notices = []
        for path in source.rglob("*"):
            relative = path.relative_to(source)
            lower = path.name.lower()
            if not path.is_file() or path.is_symlink():
                continue
            if (lower.startswith(("license", "licence", "copying", "notice"))
                    or lower in {"ofl.txt", "ufl.txt", "hack-regular.txt"}):
                notices.append((path, relative))
        if package.get("license_file"):
            path = (source / package["license_file"]).resolve()
            if path.is_file() and path.is_relative_to(source):
                notices.append((path, path.relative_to(source)))
        for path, relative in notices:
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
        # dispatch's published source declares MIT and its author, but does not
        # carry a separate license file. Preserve that declaration and provide
        # the MIT text explicitly, rather than silently omitting the component.
        if not notices and f"{name}@{version}" not in supplemental:
            if name != "dispatch" or version != "0.2.0":
                raise SystemExit(f"Missing license notice for {name} {version}")
            mit = (ROOT / "LICENSE").read_text().split("Permission is hereby", 1)[1]
            (destination / "LICENSE-MIT.txt").write_text(
                "MIT License\n\nAuthor: Steven Sheldon\n"
                "License declared by dispatch 0.2.0 Cargo.toml.\n\n"
                "Permission is hereby" + mit)
        url = f"https://static.crates.io/crates/{name}/{name}-{version}.crate"
        index.append("\t".join((name, version, package.get("license") or "SEE NOTICE",
                                url, checksums[(name, version)])))
    (output / "SOURCES.tsv").write_text("\n".join(index) + "\n")
    (output / "README.txt").write_text(
        "Third-party components retain their respective licenses and notices.\n"
        "SOURCES.tsv gives the exact, unmodified published Rust sources and their\n"
        "Cargo.lock SHA-256 checksums, including the MPL-2.0 Symphonia components.\n"
        "Upstream notices absent from crate archives are retained under upstream/\n"
        "with pinned source URLs in manifest.json. The dependency graph includes\n"
        "development dependencies as a conservative superset. LMS is excluded.\n")
    print(f"Collected notices and source references for {len(packages)} dependencies")


if __name__ == "__main__":
    main()
