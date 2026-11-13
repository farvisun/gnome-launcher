#!/usr/bin/env python3
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

root = Path(__file__).resolve().parents[1]
dist = root / 'dist'
dist.mkdir(exist_ok=True)
binary = root / 'target/release/gnome-launcher'
if not binary.exists():
    raise SystemExit('Build first: cargo build --release --locked')
version = tomllib.loads((root / 'Cargo.toml').read_text())['package']['version']
arch = subprocess.check_output(['dpkg', '--print-architecture'], text=True).strip()
with tempfile.TemporaryDirectory(prefix='lumen-deb-') as temporary:
    stage = Path(temporary)
    def copy(source, target):
        destination = stage / target
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
    copy(binary, 'usr/bin/gnome-launcher')
    for source, destination in [
        ('io.github.gnomelauncher.Launcher.desktop', 'usr/share/applications'),
        ('io.github.gnomelauncher.Launcher.service', 'usr/share/dbus-1/services'),
        ('io.github.gnomelauncher.Launcher.svg', 'usr/share/icons/hicolor/scalable/apps'),
    ]:
        copy(root / 'data' / source, f'{destination}/{source}')
    copy(root / 'data/lumen-autostart.desktop', 'etc/xdg/autostart/lumen.desktop')
    # Files reports its selection to Image Modification through nautilus-python.
    copy(root / 'data/nautilus/lumen-images.py', 'usr/share/nautilus-python/extensions/lumen-images.py')
    # Built-in extensions keep the repository layout so `../../sdk/index.mjs` resolves.
    builtin = stage / 'usr/share/gnome-launcher/builtin'
    shutil.copytree(root / 'extensions', builtin / 'extensions', ignore=shutil.ignore_patterns('*.test.mjs'))
    copy(root / 'sdk/index.mjs', 'usr/share/gnome-launcher/builtin/sdk/index.mjs')
    shell = stage / 'usr/share/gnome-shell/extensions/lumen@gnome-launcher.local'
    shutil.copytree(root / 'shell', shell)
    subprocess.run(['glib-compile-schemas', '--strict', str(shell / 'schemas')], check=True)
    (stage / 'DEBIAN').mkdir()
    (stage / 'DEBIAN/control').write_text(f'''Package: lumen-launcher
Version: {version}
Architecture: {arch}
Maintainer: Lumen contributors
Depends: libgtk-4-1 (>= 4.12), libadwaita-1-0 (>= 1.5), libglib2.0-0t64, gnome-shell (>= 50), gnome-shell (<< 51), localsearch, libtinysparql-3.0-0, nodejs (>= 22)
Recommends: libnotify-bin, pkexec, psmisc, imagemagick, python3-nautilus
Suggests: potrace, libimage-exiftool-perl
Section: utils
Priority: optional
Description: Native extensible GNOME command launcher
 Search applications and files, calculate expressions, and run JS/TS extensions.
''')
    output = dist / f'lumen-launcher_{version}_{arch}.deb'
    subprocess.run(['dpkg-deb', '--root-owner-group', '--build', str(stage), str(output)], check=True)
    print(f'{output}\nAfter installation: gnome-extensions enable lumen@gnome-launcher.local (log out/in once if needed).')
