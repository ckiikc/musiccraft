#!/usr/bin/env python3
"""Stage cross-platform packages from matching binaries. Never install or publish."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib
import zipfile
ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']

def run(args):
    subprocess.run(list(map(str,args)),cwd=ROOT,check=True)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('platform',choices=['linux','windows'])
    parser.add_argument('--target',required=True)
    parser.add_argument('--profile',default='release',choices=['release','debug'])
    parser.add_argument('--makensis',default='makensis')
    options=parser.parse_args()
    target=Path(os.environ.get('CARGO_TARGET_DIR',ROOT/'target')).resolve()
    executable=target/options.target/options.profile/('musiccraft.exe' if options.platform=='windows' else 'musiccraft')
    if not executable.is_file():
        raise SystemExit(f'Matching target binary missing: {executable.name}; build with --target {options.target}')
    arch={'x86_64':'x86_64','aarch64':'arm64'}[options.target.split('-')[0]]
    name=f'musiccraft-{VERSION}-{options.platform}-{arch}-experimental'
    stage=ROOT/'dist'/name
    if stage.is_symlink():raise SystemExit('Refusing generated symlink')
    if stage.exists():shutil.rmtree(stage)
    stage.mkdir(parents=True)
    shutil.copy2(executable,stage/executable.name)
    for item in ['LICENSE','THIRD_PARTY.md','ATTRIBUTION.md']:
        shutil.copy2(ROOT/item,stage/item)
    runtime=ROOT/'vendor'/f'codecs-{options.platform}'
    for tool in ['ffmpeg','ffprobe']:
        if not (runtime/(tool+('.exe' if options.platform=='windows' else ''))).is_file():
            raise SystemExit('Build the matching codec runtime before packaging')
    shutil.copytree(runtime,stage/'codecs')
    run(['python3','scripts/collect-licenses.py',stage/'ThirdPartyLicenses','--target',options.target])
    (stage/'README.txt').write_text(
        f'MusicCraft {VERSION} — experimental {options.platform} {arch}\n'
        'Independent music player. https://github.com/ckiikc/musiccraft\n'
        'Local/NAS, FTP/FTPS and paired MusicCraft Android libraries. No personal data included.\n'
        'User data and credentials use OS-specific directories/stores; updates preserve them.\n'
        'DoP is opt-in, requires a compatible DSD DAC and exclusive integer output at unity hardware volume.\n'
        'DSD64 needs 176.4 kHz; DSD128 352.8 kHz; higher rates require matching DAC support.\n'
        'DoP never uses float, DSP or resampling. Native ASIO DSD and physical DAC certification are not included.\n'
        'DSF and uncompressed DSDIFF support DoP. Compressed DST uses PCM conversion.\n'
        'WMA with DRM is unsupported. WavPack hybrid correction files are not applied; strict mode rejects hybrid.\n'
        'Extra codecs use the bundled minimal LGPL FFmpeg 8.0.1 runtime; sources/license/build recipe are in codecs/licenses.\n'
        'For extra-codec remote files, the original authenticated download is cached before decoding (1 GiB/file, 2 GiB cache).\n'
        'DoP DSF/DFF instead use progressive authenticated reads and bounded memory.\n'
        'The optional LMS runtime is excluded; external LMS control is included.\n'
        + ('Windows 10/11 64-bit. Per-user unsigned installer; no administrator required.\n'
           'Choose MusicCraft in Windows Default apps/Open with; existing defaults are never overwritten.\n'
           'Local-network discovery may require the Windows private-network firewall prompt.\n'
           if options.platform=='windows' else
           'Debian 12 / Ubuntu 24.04 or newer, x86-64, X11 or Wayland.\n'
           'Use the .deb with your package manager, or ./install.sh for a per-user install (Python 3 needed).\n'
           'A Secret Service provider (e.g. GNOME Keyring/KWallet), a D-Bus desktop session and an XDG file portal are required.\n'
           'ALSA direct hw output is used for exclusive playback; sound-server/default/plug devices remain shared.\n'
           'Wayland manages placement/pinning; global window coordinates cannot be forced by applications.\n'),encoding='utf-8')
    artifacts=[]
    if options.platform=='windows':
        icons=list((target/options.target/options.profile/'build').glob('musiccraft-*/out/musiccraft.ico'))
        if not icons:raise SystemExit('Windows icon resource missing')
        icon=max(icons,key=lambda p:p.stat().st_mtime)
        output=ROOT/'dist'/f'{name}-setup.exe'
        prefix='/D' if os.name=='nt' else '-D'
        run([options.makensis,f'{prefix}VERSION={VERSION}',f'{prefix}STAGE={stage}',f'{prefix}ICON={icon}',f'{prefix}OUTPUT={output}',ROOT/'packaging/windows.nsi'])
        artifacts.append(output)
        output=ROOT/'dist'/f'{name}-portable.zip'
        with zipfile.ZipFile(output,'w',zipfile.ZIP_DEFLATED,compresslevel=9) as archive:
            for path in sorted(stage.rglob('*')):
                if path.is_file():archive.write(path,Path(name)/path.relative_to(stage))
        artifacts.append(output)
    else:
        for source,dest in [('packaging/install-linux.sh','install.sh'),('packaging/musiccraft.desktop','musiccraft.desktop'),('packaging/musiccraft.xml','musiccraft.xml'),('assets/app-icon/musiccraft-1024.png','musiccraft.png')]:
            shutil.copy2(ROOT/source,stage/dest)
        output=ROOT/'dist'/f'{name}.tar.gz'
        with tarfile.open(output,'w:gz') as archive:
            archive.add(stage,arcname=name,filter=lambda info: normalize(info))
        artifacts.append(output)
        package=ROOT/'dist'/f'deb-{arch}'
        if package.exists():shutil.rmtree(package)
        payload=package/'usr/lib/musiccraft';payload.mkdir(parents=True)
        for path in stage.iterdir():
            if path.name in {'install.sh','musiccraft.desktop','musiccraft.xml','musiccraft.png'}:continue
            if path.is_dir():shutil.copytree(path,payload/path.name)
            else:shutil.copy2(path,payload/path.name)
        for source,dest in [('packaging/musiccraft.desktop','usr/share/applications/io.github.ckiikc.musiccraft.desktop'),('packaging/musiccraft.xml','usr/share/mime/packages/io.github.ckiikc.musiccraft.xml'),('assets/app-icon/musiccraft-1024.png','usr/share/icons/hicolor/1024x1024/apps/io.github.ckiikc.musiccraft.png')]:
            out=package/dest;out.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/source,out)
        launcher=package/'usr/bin/musiccraft';launcher.parent.mkdir(parents=True,exist_ok=True);launcher.write_text('#!/bin/sh\nexec /usr/lib/musiccraft/musiccraft "$@"\n');launcher.chmod(0o755)
        control=package/'DEBIAN';control.mkdir()
        (control/'control').write_text(f'Package: musiccraft\nVersion: {VERSION}\nArchitecture: {"amd64" if arch=="x86_64" else "arm64"}\nMaintainer: MusicCraft contributors <1676293+ckiikc@users.noreply.github.com>\nSection: sound\nPriority: optional\nDepends: libc6 (>= 2.36), libasound2, libdbus-1-3, libudev1, libx11-6, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libgl1, libegl1, libssl3, libcurl4, curl, ca-certificates, xdg-desktop-portal\nRecommends: gnome-keyring, xdg-desktop-portal-gtk, fonts-noto-core, fonts-noto-cjk\nHomepage: https://github.com/ckiikc/musiccraft\nDescription: MusicCraft music player (experimental)\n Local, NAS, FTP and paired Android libraries with exclusive PCM/DoP output.\n')
        for script in ['postinst','postrm']:
            path=control/script;path.write_text('#!/bin/sh\nset -e\nif command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database -q; fi\nif command -v update-mime-database >/dev/null 2>&1; then update-mime-database /usr/share/mime; fi\n');path.chmod(0o755)
        output=ROOT/'dist'/f'musiccraft_{VERSION}_{"amd64" if arch=="x86_64" else "arm64"}.deb'
        run(['dpkg-deb','--root-owner-group','--build',package,output]);artifacts.append(output)
    for artifact in artifacts:
        digest=hashlib.sha256(artifact.read_bytes()).hexdigest()
        artifact.with_suffix(artifact.suffix+'.sha256').write_text(f'{digest}  {artifact.name}\n')
        print(f'{artifact.name}: {digest}')

def normalize(info):
    info.uid=info.gid=0;info.uname=info.gname='root';return info
if __name__=='__main__':main()
