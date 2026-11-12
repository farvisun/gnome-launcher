#!/usr/bin/env python3
"""Optional Ubuntu build headers without root. Runtime libraries must be installed."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1] / '.build-deps'
debs = root / 'debs'
debs.mkdir(parents=True, exist_ok=True)
packages = ['libgtk-4-dev', 'libadwaita-1-dev', 'libpango1.0-dev',
            'libgraphene-1.0-dev', 'libgdk-pixbuf-2.0-dev', 'libharfbuzz-dev',
            'libgraphite2-dev', 'libfribidi-dev', 'libthai-dev', 'libxft-dev',
            'libglycin-2-dev', 'libvulkan-dev', 'libappstream-dev', 'libdatrie-dev',
            'libxml2-dev', 'libseccomp-dev', 'liblcms2-dev']
subprocess.run(['apt-get', 'download', *packages], cwd=debs, check=True)
for package in debs.glob('*.deb'):
    subprocess.run(['dpkg-deb', '-x', str(package), str(root / 'root')], check=True)
for pc in (root / 'root/usr/lib/x86_64-linux-gnu/pkgconfig').glob('*.pc'):
    pc.write_text(pc.read_text().replace('prefix=/usr', f'prefix={root}/root/usr'))
for link in (root / 'root/usr/lib/x86_64-linux-gnu').glob('*.so'):
    if link.is_symlink() and not link.exists():
        target = Path('/usr/lib/x86_64-linux-gnu') / link.readlink().name
        if target.exists():
            link.unlink()
            link.symlink_to(target)
print('Build with: bash -c "source scripts/dev-env.sh; cargo build --locked"')
