#!/usr/bin/env python3
"""Remove the per-user installation while retaining preferences and extensions."""
from pathlib import Path
import os
import shutil
import subprocess
import json

home = Path.home()
data = Path(os.environ.get('XDG_DATA_HOME', home / '.local/share'))
config = Path(os.environ.get('XDG_CONFIG_HOME', home / '.config'))
extension = data / 'gnome-shell/extensions/lumen@gnome-launcher.local'
subprocess.run(['gnome-extensions', 'disable', 'lumen@gnome-launcher.local'], capture_output=True)
# Restore the journal even if the Shell extension is unavailable or never loaded.
if extension.exists():
    import gi
    from gi.repository import Gio
    source = Gio.SettingsSchemaSource.new_from_directory(str(extension / 'schemas'), Gio.SettingsSchemaSource.get_default(), False)
    schema = source.lookup('org.gnome.shell.extensions.lumen', False)
    settings = Gio.Settings.new_full(schema, None, None)
    saved = settings.get_string('shortcut-backup')
    if saved:
        for entry in json.loads(saved):
            target = Gio.Settings.new(entry['schema'])
            if list(target.get_strv(entry['key'])) == entry['after']:
                target.set_strv(entry['key'], entry['before'])
        settings.set_string('shortcut-backup', '')
    shell = Gio.Settings.new('org.gnome.shell')
    shell.set_strv('enabled-extensions', [v for v in shell.get_strv('enabled-extensions') if v != 'lumen@gnome-launcher.local'])
    Gio.Settings.sync()
binary = home / '.local/bin/gnome-launcher'
if binary.exists():
    subprocess.run([str(binary), '--quit'], capture_output=True)
for target in [binary, data / 'applications/io.github.gnomelauncher.Launcher.desktop', data / 'dbus-1/services/io.github.gnomelauncher.Launcher.service', data / 'icons/hicolor/scalable/apps/io.github.gnomelauncher.Launcher.svg', config / 'autostart/lumen.desktop']:
    target.unlink(missing_ok=True)
if extension.exists():
    shutil.rmtree(extension)
# Only the bundled extension code; user data and extension storage stay.
builtin = home / '.local/share/gnome-launcher/builtin'
if builtin.exists():
    shutil.rmtree(builtin)
print(f'Lumen uninstalled. Preferences and extension data retained at {data / "gnome-launcher"}.')
