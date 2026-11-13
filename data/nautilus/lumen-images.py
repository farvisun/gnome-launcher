"""Lumen's Files integration for Image Modification.

Files asks menu providers for their items whenever the selection changes
(nautilus_files_view_notify_selection_changed → update_extensions_menus), so
writing the selection here lets Lumen act on "the images selected in Files",
as Raycast does with Finder. It also adds an "Image Modification" submenu for
the commands that need no arguments.
"""
import json
import os

import gi

gi.require_version('Nautilus', '4.0')
from gi.repository import GLib, GObject, Gio, Nautilus  # noqa: E402

STATE = os.path.join(GLib.get_user_runtime_dir(), 'gnome-launcher', 'nautilus-selection.json')
APP = 'io.github.gnomelauncher.Launcher'
PATH = '/io/github/gnomelauncher/Launcher'
COMMANDS = [
    ('flip-horizontally', 'Flip Horizontally'),
    ('flip-vertically', 'Flip Vertically'),
    ('convert', 'Convert…'),
    ('apply-filter', 'Apply Filter…'),
    ('optimize', 'Optimize'),
    ('strip-exif', 'Strip Exif Data'),
    ('remove-bg', 'Remove Background'),
]


def _path(file_info):
    location = file_info.get_location()
    return location.get_path() if location is not None else None


class LumenImages(GObject.GObject, Nautilus.MenuProvider):
    def __init__(self):
        super().__init__()
        self._files = []
        self._folder = None

    def _save(self):
        try:
            os.makedirs(os.path.dirname(STATE), mode=0o700, exist_ok=True)
            temporary = f'{STATE}.{os.getpid()}.tmp'
            with open(temporary, 'w') as out:
                json.dump({'pid': os.getpid(), 'files': self._files, 'folder': self._folder}, out)
            os.replace(temporary, STATE)
        except OSError:
            pass

    def _run(self, _item, command, files):
        target = f'lumen.sips/{command}#{json.dumps({"files": files})}'
        Gio.bus_get_sync(Gio.BusType.SESSION, None).call(
            APP, PATH, 'org.gtk.Actions', 'Activate',
            GLib.Variant('(sava{sv})', ('run-command', [GLib.Variant('s', target)], {})),
            None, Gio.DBusCallFlags.NONE, 5000, None, None)

    def get_file_items(self, files):
        self._files = [p for p in (_path(f) for f in files) if p]
        self._save()
        images = [_path(f) for f in files if _path(f) and (f.get_mime_type() or '').startswith('image/')]
        if not images or len(images) != len(files):
            return []
        top = Nautilus.MenuItem(name='Lumen::ImageModification', label='Image Modification')
        submenu = Nautilus.Menu()
        top.set_submenu(submenu)
        for command, label in COMMANDS:
            item = Nautilus.MenuItem(name=f'Lumen::{command}', label=label)
            item.connect('activate', self._run, command, images)
            submenu.append_item(item)
        return [top]

    def get_background_items(self, folder):
        path = _path(folder)
        if path != self._folder:
            self._folder = path
            self._save()
        return []
