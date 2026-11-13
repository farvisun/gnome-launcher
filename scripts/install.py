#!/usr/bin/env python3
"""Install a built Lumen into this user's XDG directories; no root required."""
from pathlib import Path
import os
import shutil
import subprocess
import sys
import time
import gi
from gi.repository import Gio, GLib

root = Path(__file__).resolve().parents[1]
home = Path.home()
data = Path(os.environ.get('XDG_DATA_HOME', home / '.local/share'))
config = Path(os.environ.get('XDG_CONFIG_HOME', home / '.config'))
binary = root / 'target/release/gnome-launcher'
if '--debug' in sys.argv:
    binary = root / 'target/debug/gnome-launcher'
if not binary.is_file():
    sys.exit('Build first: cargo build --release --locked')
destination = home / '.local/bin/gnome-launcher'
destination.parent.mkdir(parents=True, exist_ok=True)
bus = Gio.bus_get_sync(Gio.BusType.SESSION)
def owner():
    return bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'NameHasOwner', GLib.Variant('(s)', ('io.github.gnomelauncher.Launcher',)), GLib.VariantType.new('(b)'), Gio.DBusCallFlags.NONE, 3000, None).unpack()[0]

if owner():
    # Also stop a development build using the same application ID.
    subprocess.run([str(binary), '--quit'], check=True)
    deadline = time.monotonic() + 5
    while owner() and time.monotonic() < deadline:
        time.sleep(0.05)
    if owner():
        sys.exit('The previous launcher did not stop; installation was left unchanged.')
temporary = destination.with_suffix('.new')
shutil.copy2(binary, temporary)
temporary.chmod(0o755)
temporary.replace(destination)

def install_template(source, target):
    target.parent.mkdir(parents=True, exist_ok=True)
    text = (root / 'data' / source).read_text()
    # Desktop and D-Bus Exec values use double-quoted executable paths.
    quoted = '"' + str(destination).replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$') + '"'
    target.write_text(text.replace('Exec=gnome-launcher', 'Exec=' + quoted))

install_template('io.github.gnomelauncher.Launcher.desktop', data / 'applications/io.github.gnomelauncher.Launcher.desktop')
install_template('io.github.gnomelauncher.Launcher.service', data / 'dbus-1/services/io.github.gnomelauncher.Launcher.service')
install_template('lumen-autostart.desktop', config / 'autostart/lumen.desktop')
icon = data / 'icons/hicolor/scalable/apps/io.github.gnomelauncher.Launcher.svg'
icon.parent.mkdir(parents=True, exist_ok=True)
shutil.copy2(root / 'data/io.github.gnomelauncher.Launcher.svg', icon)
# Built-in extensions sit beside the binary's prefix (~/.local), where Lumen looks
# for them, and keep the repository layout so `../../sdk/index.mjs` resolves.
builtin = destination.parent.parent / 'share/gnome-launcher/builtin'
if builtin.exists():
    shutil.rmtree(builtin)
shutil.copytree(root / 'extensions', builtin / 'extensions', ignore=shutil.ignore_patterns('*.test.mjs'))
(builtin / 'sdk').mkdir(parents=True)
shutil.copy2(root / 'sdk/index.mjs', builtin / 'sdk/index.mjs')
extension = data / 'gnome-shell/extensions/lumen@gnome-launcher.local'
extension.mkdir(parents=True, exist_ok=True)
shutil.copytree(root / 'shell', extension, dirs_exist_ok=True)
subprocess.run(['glib-compile-schemas', '--strict', str(extension / 'schemas')], check=True)
if shutil.which('update-desktop-database'):
    subprocess.run(['update-desktop-database', str(data / 'applications')], check=True)
if shutil.which('gtk-update-icon-cache'):
    subprocess.run(['gtk-update-icon-cache', '-f', '-t', str(data / 'icons/hicolor')], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
subprocess.Popen([str(destination), '--background'], stdout=subprocess.DEVNULL, stderr=open(data / 'gnome-launcher.log', 'a'), start_new_session=True)
enabled = subprocess.run(['gnome-extensions', 'enable', 'lumen@gnome-launcher.local'], capture_output=True, text=True)
if enabled.returncode:
    # Persist enablement for the next session even if Shell has not scanned this new UUID.
    settings = Gio.Settings.new('org.gnome.shell')
    uuids = list(settings.get_strv('enabled-extensions'))
    if 'lumen@gnome-launcher.local' not in uuids:
        settings.set_strv('enabled-extensions', uuids + ['lumen@gnome-launcher.local'])
    settings.set_strv('disabled-extensions', [v for v in settings.get_strv('disabled-extensions') if v != 'lumen@gnome-launcher.local'])
    Gio.Settings.sync()
    print('Installed. Log out and back in once to load the new GNOME Shell integration.')
else:
    print('Installed and enabled. Press Super+Space to open Lumen.')
print(f'Launcher: {destination}\nSettings: {destination} --settings\nLogs: {data / "gnome-launcher.log"}')
