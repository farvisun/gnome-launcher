#!/usr/bin/env python3
"""Exercise the integration in an isolated headless GNOME Shell, not the live desktop."""
from pathlib import Path
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
UUID = 'lumen@gnome-launcher.local'
DOCK_UUID = 'ubuntu-dock@ubuntu.com'
if '--session' not in sys.argv:
    with tempfile.TemporaryDirectory(prefix='lumen-shell-') as temporary:
        sandbox = Path(temporary)
        for directory in ['data', 'config', 'cache', 'runtime']:
            (sandbox / directory).mkdir(mode=0o700)
        extension = sandbox / 'data/gnome-shell/extensions' / UUID
        shutil.copytree(ROOT / 'shell', extension)
        # Inspect window-list membership through Eval only in this private Shell.
        # The installed extension never changes the live session's unsafe mode.
        with (extension / 'extension.js').open('a') as module:
            module.write('\nglobal.context.unsafe_mode = true;\n')
        service = sandbox / 'data/dbus-1/services/io.github.gnomelauncher.Launcher.service'
        service.parent.mkdir(parents=True)
        service.write_text((ROOT / 'data/io.github.gnomelauncher.Launcher.service').read_text()
                           .replace('Exec=gnome-launcher', f'Exec=/usr/bin/env WAYLAND_DISPLAY=lumen-test GDK_BACKEND=wayland XDG_RUNTIME_DIR={sandbox / "runtime"} {ROOT / "target/release/gnome-launcher"}'))
        subprocess.run(['glib-compile-schemas', '--strict', str(extension / 'schemas')], check=True)
        env = dict(os.environ, XDG_DATA_HOME=str(sandbox / 'data'), XDG_CONFIG_HOME=str(sandbox / 'config'),
                   XDG_CACHE_HOME=str(sandbox / 'cache'), XDG_RUNTIME_DIR=str(sandbox / 'runtime'),
                   GSETTINGS_BACKEND='keyfile', LIBGL_ALWAYS_SOFTWARE='1', GNOME_SHELL_SESSION_MODE='user',
                   WAYLAND_DISPLAY='lumen-test', GDK_BACKEND='wayland')
        result = subprocess.run(['dbus-run-session', '--', sys.executable, __file__, '--session'], env=env)
        sys.exit(result.returncode)

import gi
from gi.repository import Gio, GLib

settings = Gio.Settings.new('org.gnome.shell')
settings.set_strv('enabled-extensions', [DOCK_UUID, UUID])
settings.set_boolean('disable-user-extensions', False)
wm = Gio.Settings.new('org.gnome.desktop.wm.keybindings')
original = {key: list(wm.get_strv(key)) for key in ['switch-input-source', 'switch-input-source-backward']}
Gio.Settings.sync()
logpath = ROOT / 'dist/shell-smoke.log'
logpath.parent.mkdir(exist_ok=True)
log = logpath.open('w')
shell = subprocess.Popen(['gnome-shell', '--headless', '--wayland', '--no-x11', '--virtual-monitor', '1280x800', '--wayland-display', 'lumen-test'], stdout=log, stderr=subprocess.STDOUT)

def spin():
    context = GLib.MainContext.default()
    while context.pending(): context.iteration(False)
    time.sleep(0.1)

try:
    bus = Gio.bus_get_sync(Gio.BusType.SESSION)
    def call(method, signature, values):
        return bus.call_sync('org.gnome.Shell.Extensions', '/org/gnome/Shell/Extensions', 'org.gnome.Shell.Extensions', method, GLib.Variant(signature, values), None, Gio.DBusCallFlags.NONE, 2000, None).unpack()
    deadline = time.monotonic() + 25
    while True:
        if shell.poll() is not None:
            raise RuntimeError(f'Nested Shell stopped with {shell.returncode}: {logpath.read_text()[-6000:]}')
        try:
            info = call('GetExtensionInfo', '(s)', (UUID,))[0]
            dock_info = call('GetExtensionInfo', '(s)', (DOCK_UUID,))[0]
            if info.get('state') == 1 and dock_info.get('state') == 1: break
            if info.get('error'): raise RuntimeError(info['error'])
            if dock_info.get('error'): raise RuntimeError(dock_info['error'])
        except GLib.Error:
            pass
        if time.monotonic() > deadline: raise RuntimeError(f'Extension did not activate: {logpath.read_text()[-6000:]}')
        spin()
    for _ in range(5): spin()
    forward = list(wm.get_strv('switch-input-source'))
    backward = list(wm.get_strv('switch-input-source-backward'))
    assert '<Super>space' not in forward, forward
    assert '<Super><Shift>space' in forward, forward
    assert '<Shift><Super>space' not in backward, backward
    # Deliver the actual accelerator through Mutter's virtual keyboard in this
    # private compositor. No input is injected into the user's desktop.
    desktop = Path(os.environ['XDG_DATA_HOME']) / 'applications/io.github.gnomelauncher.Launcher.desktop'
    desktop.parent.mkdir(parents=True, exist_ok=True)
    binary = ROOT / 'target/release/gnome-launcher'
    desktop.write_text((ROOT / 'data/io.github.gnomelauncher.Launcher.desktop').read_text().replace('Exec=gnome-launcher', f'Exec={binary}'))
    # Harmless applications prove that keyboard activation launches the selected
    # result, without opening windows or changing anything on the live desktop.
    opened = {}
    for name in ['Alpha', 'Beta']:
        opened[name] = Path(os.environ['XDG_DATA_HOME']) / f'enter-check-{name.lower()}'
        desktop.with_name(f'lumen-enter-check-{name.lower()}.desktop').write_text(
            f'[Desktop Entry]\nType=Application\nName=Lumen Enter Check {name}\n'
            f'Exec={shutil.which("touch")} {opened[name]}\nTerminal=false\n')
    # Turn on the built-ins exercised below before the launcher reads its state.
    state_file = Path(os.environ['XDG_DATA_HOME']) / 'gnome-launcher/state.json'
    state_file.parent.mkdir(parents=True, exist_ok=True)
    state_file.write_text(json.dumps({'enabled_builtins': ['lumen.password-generator', 'lumen.weather', 'lumen.sips'],
                                      'preferences': {'lumen.weather': {'defaultquery': 'Berlin', 'clockformat': '24h'},
                                                      'lumen.sips': {'inputMethod': 'Clipboard'}}}))
    app_log = (ROOT / 'dist/shell-app.log').open('w')
    app = subprocess.Popen([str(binary), '--background'], env=dict(os.environ, WAYLAND_DISPLAY='lumen-test', GDK_BACKEND='wayland', LUMEN_PROFILE='1', LUMEN_BUILTIN_DIR=str(ROOT / 'extensions')), stdout=app_log, stderr=subprocess.STDOUT)
    try:
        def invoke(destination, path, interface, method, signature='()', values=()):
            return bus.call_sync(destination, path, interface, method, GLib.Variant(signature, values), None, Gio.DBusCallFlags.NONE, 3000, None).unpack()
        def shell_eval(expression):
            ok, value = invoke('org.gnome.Shell', '/org/gnome/Shell', 'org.gnome.Shell', 'Eval', '(s)', (expression,))
            assert ok, value
            return json.loads(value)
        def palette_state():
            return shell_eval('''({
                windows: global.display.list_all_windows()
                    .filter(window => window.get_gtk_application_id() === 'io.github.gnomelauncher.Launcher' && window.get_title() === 'Lumen Launcher')
                    .map(window => ({skipTaskbar: window.is_skip_taskbar(), focused: global.display.focus_window === window})),
                listed: imports.gi.Shell.AppSystem.get_default().get_running()
                    .some(app => app.get_id() === 'io.github.gnomelauncher.Launcher.desktop' || app.get_windows()
                        .some(window => window.get_gtk_application_id() === 'io.github.gnomelauncher.Launcher'
                            || window.get_wm_class() === 'io.github.gnomelauncher.Launcher'))
            })''')
        def assert_palette_unlisted():
            state = palette_state()
            assert state['windows'] and all(window['skipTaskbar'] for window in state['windows']), state
            assert any(window['focused'] for window in state['windows']), state
            assert not state['listed'], f'Palette is still listed as a running dock application: {state}'
        for _ in range(10): spin()
        shell_eval('''(async () => {
            const {dockManager} = await import('file:///usr/share/gnome-shell/extensions/ubuntu-dock@ubuntu.com/extension.js');
            if (!dockManager?._allDocks.length) throw new Error('Ubuntu Dock did not create a dock');
            const trace = global._lumenDockTrace = {icons: [], listedAtSignal: []};
            const appSystem = imports.gi.Shell.AppSystem.get_default();
            const describe = app => ({id: app.get_id(), windows: app.get_windows().map(window => ({
                pid: window.get_pid(), appId: window.get_gtk_application_id(),
                wmClass: window.get_wm_class(), title: window.get_title()}))});
            appSystem.connect('app-state-changed', (_system, app) => {
                trace.listedAtSignal.push({...describe(app), state: app.state,
                    listed: appSystem.get_running().includes(app)});
            });
            for (const dock of dockManager._allDocks) {
                dock.dash._box.connect('child-added', (_box, child) => {
                    const app = child.child?._delegate?.app;
                    if (app) trace.icons.push(describe(app));
                });
            }
            return true;
        })()''')
        remote_path = invoke('org.gnome.Mutter.RemoteDesktop', '/org/gnome/Mutter/RemoteDesktop', 'org.gnome.Mutter.RemoteDesktop', 'CreateSession')[0]
        remote = lambda method, signature='()', values=(): invoke('org.gnome.Mutter.RemoteDesktop', remote_path, 'org.gnome.Mutter.RemoteDesktop.Session', method, signature, values)
        remote('Start')
        # XKB keysyms: Super_L=0xffeb, space=0x20.
        for symbol, pressed in [(0xffeb, True), (0x20, True), (0x20, False), (0xffeb, False)]:
            remote('NotifyKeyboardKeysym', '(ub)', (symbol, pressed))
        for _ in range(10): spin()
        assert app.poll() is None, (ROOT / 'dist/shell-app.log').read_text()
        assert 'Lumen present requested' in (ROOT / 'dist/shell-app.log').read_text(), 'Super+Space did not open the palette'
        assert '_position@' not in logpath.read_text(), 'Shell window placement failed'
        assert_palette_unlisted()
        group = Gio.DBusActionGroup.get(bus, 'io.github.gnomelauncher.Launcher', '/io/github/gnomelauncher/Launcher')
        group.list_actions()  # Start the proxy's lazy asynchronous initialization.
        for _ in range(5): spin()
        assert group.has_action('toggle'), 'Launcher did not export its activation action'
        # Watch six openings, including all intermediate app-state changes.
        for _ in range(10):
            for symbol, pressed in [(0xffeb, True), (0x20, True), (0x20, False), (0xffeb, False)]:
                remote('NotifyKeyboardKeysym', '(ub)', (symbol, pressed))
            for _ in range(3): spin()
        import re
        timings = re.findall(r'present requested in ([0-9.]+) ms', (ROOT / 'dist/shell-app.log').read_text())
        assert len(timings) >= 6, 'Repeated Super+Space did not reopen the palette'
        assert_palette_unlisted()
        print(f'Application-side presentation: cold {timings[0]} ms, warm {timings[-1]} ms')
        # Leave queued map/position callbacks behind while old GTK toplevels are
        # destroyed. They must never move an unmanaged Meta.Window.
        for _ in range(20):
            for symbol, pressed in [(0xffeb, True), (0x20, True), (0x20, False), (0xffeb, False)]:
                remote('NotifyKeyboardKeysym', '(ub)', (symbol, pressed))
        group.activate_action('show', None)
        for _ in range(5): spin()
        assert shell.poll() is None, 'GNOME Shell crashed during rapid reopening'
        assert_palette_unlisted()
        trace = shell_eval('global._lumenDockTrace')
        assert not trace['icons'], f'Opening the palette inserted dock icons, even if later removed: {trace}'
        assert trace['listedAtSignal'] and not any(event['listed'] for event in trace['listedAtSignal']), f'Palette was briefly exposed to dock consumers: {trace}'
        print('PASS: no dock icon is created and no app-state notification exposes the palette during opening')
        print('PASS: rapid opening and closing leaves no stale window-positioning callbacks')
        def press(symbol):
            for pressed in [True, False]:
                remote('NotifyKeyboardKeysym', '(ub)', (symbol, pressed))
        def type_query(query):
            for character in query: press(ord(character))
            for _ in range(5): spin()
        def wait_opened(name, key):
            deadline = time.monotonic() + 3
            while not opened[name].exists() and time.monotonic() < deadline: spin()
            assert opened[name].exists(), f'{key} did not launch the selected {name} result'
        type_query('Lumen Enter Check')
        press(0xff54)  # Down selects Beta; focus remains in the search entry.
        press(0xff0d)  # Return.
        wait_opened('Beta', 'Enter after Down')
        assert not opened['Alpha'].exists(), 'Enter launched the first result instead of the selected one'
        for symbol, pressed in [(0xffeb, True), (0x20, True), (0x20, False), (0xffeb, False)]:
            remote('NotifyKeyboardKeysym', '(ub)', (symbol, pressed))
        for _ in range(3): spin()
        type_query('Lumen Enter Check Alpha')
        press(0xff8d)  # Keypad Enter activates the initial selection without navigation.
        wait_opened('Alpha', 'Keypad Enter')
        print('PASS: Enter launches the arrow-selected result; keypad Enter launches the initial selection')
        def stage_has_text(text):
            return shell_eval('''(() => {
                const find = actor => actor.text === %s || actor.get_children().some(find);
                return find(global.stage);
            })()''' % json.dumps(text))
        invoke('org.gnome.Shell', '/io/github/gnomelauncher/Shell', 'io.github.gnomelauncher.Shell', 'ShowHud', '(ss)', ('Smoke HUD', ''))
        for _ in range(3): spin()
        assert stage_has_text('Smoke HUD'), 'ShowHud did not draw the HUD'
        print('PASS: the Shell draws HUD messages sent over D-Bus')
        # A no-view command copies, closes Lumen and shows its HUD.
        group.activate_action('run-command', GLib.Variant('s', 'lumen.password-generator/copy-random-password'))
        deadline = time.monotonic() + 8
        while not stage_has_text('Copied Password 🎉') and time.monotonic() < deadline: spin()
        assert stage_has_text('Copied Password 🎉'), (ROOT / 'dist/shell-app.log').read_text()[-3000:]
        for _ in range(5): spin()
        assert not any(window['focused'] for window in palette_state()['windows']), 'A no-view command left the palette open'
        copied = shell_eval('''new Promise(resolve => imports.gi.St.Clipboard.get_default()
            .get_text(imports.gi.St.ClipboardType.CLIPBOARD, (_clipboard, text) => resolve(text)))''')
        assert copied and len(copied) == 16, f'Clipboard holds {copied!r}, not a 16-character password'
        print('PASS: a no-view command copies its password, closes Lumen and shows a HUD')
        # Image Modification by keyboard: inline argument, clipboard image in, HUD and image out.
        magick = os.environ.get('LUMEN_MAGICK') or shutil.which('magick')
        if magick:
            sample = Path(os.environ['XDG_DATA_HOME']) / 'clipboard-sample.png'
            subprocess.run([magick, '-size', '40x20', 'gradient:red-blue', str(sample)], check=True)
            shell_eval('''(() => {
                const [, bytes] = imports.gi.GLib.file_get_contents(%s);
                imports.gi.St.Clipboard.get_default().set_content(imports.gi.St.ClipboardType.CLIPBOARD, 'image/png', new imports.gi.GLib.Bytes(bytes));
                return true;
            })()''' % json.dumps(str(sample)))
            group.activate_action('show', None)
            for _ in range(5): spin()
            type_query('Rotate Images')
            press(0xff09)  # Tab moves into the Angle argument.
            type_query('90')
            press(0xff0d)
            deadline = time.monotonic() + 15
            while not stage_has_text('Rotated 1 image') and time.monotonic() < deadline: spin()
            assert stage_has_text('Rotated 1 image'), (ROOT / 'dist/shell-app.log').read_text()[-3000:]
            types = shell_eval('imports.gi.St.Clipboard.get_default().get_mimetypes(imports.gi.St.ClipboardType.CLIPBOARD)')
            assert 'image/png' in types, types
            print('PASS: a command argument typed inline rotates the clipboard image and puts the result back')
        else:
            print('NOTE: ImageMagick is not installed; skipped the Image Modification check')
        Gio.Settings.new_full(Gio.SettingsSchemaSource.new_from_directory(
            str(Path(os.environ['XDG_DATA_HOME']) / 'gnome-shell/extensions' / UUID / 'schemas'),
            Gio.SettingsSchemaSource.get_default(), False).lookup('org.gnome.shell.extensions.lumen', False),
            None, None).set_boolean('show-weather-indicator', True)
        weather = '''(async () => {
            const Main = await import('resource:///org/gnome/shell/ui/main.js');
            const button = Main.panel.statusArea['lumen-weather'];
            return button ? {label: button._label.text, icon: button._icon.icon_name, items: button.menu._getMenuItems().length} : null;
        })()'''
        deadline = time.monotonic() + 30
        state = None
        while time.monotonic() < deadline:
            state = shell_eval(weather)
            if state and state['label']: break
            spin()
        assert state, 'The weather indicator did not appear in the top bar'
        if state['label']:
            assert '°C' in state['label'] and state['items'] > 10, state
            print(f'PASS: the top bar shows the weather ({state["label"]}, {state["items"]} menu rows)')
        else:
            print(f'NOTE: the weather indicator appeared but wttr.in did not answer: {state}')
        control = subprocess.Popen([sys.executable, '-c', '''import gi
gi.require_version('Gtk', '4.0')
from gi.repository import Gtk
app = Gtk.Application(application_id='io.github.gnomelauncher.DockControl')
def activate(app):
    window = Gtk.ApplicationWindow(application=app, title='Dock control')
    window.present()
app.connect('activate', activate)
app.run(None)
'''], env=dict(os.environ, WAYLAND_DISPLAY='lumen-test', GDK_BACKEND='wayland'), stdout=app_log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                state = shell_eval(f'''(async () => {{
                    const {{dockManager}} = await import('file:///usr/share/gnome-shell/extensions/ubuntu-dock@ubuntu.com/extension.js');
                    const matches = app => app && app.get_windows().some(window => window.get_pid() === {control.pid});
                    return {{listed: imports.gi.Shell.AppSystem.get_default().get_running().some(matches),
                        icon: dockManager._allDocks.some(dock => dock.dash._box.get_children()
                            .some(child => matches(child.child?._delegate?.app)))}};
                }})()''')
                if state['listed'] and state['icon']: break
                spin()
            assert state['listed'] and state['icon'], f'Normal application was hidden from the dock: {state}'
        finally:
            control.terminate(); control.wait(timeout=5)
        group.activate_action('settings', None)
        for _ in range(10): spin()
        assert palette_state()['listed'], 'Explicitly opened Lumen Settings should remain a normal app window'
        shell_eval('''global.get_window_actors().map(actor => actor.meta_window)
            .filter(window => window.get_gtk_application_id() === 'io.github.gnomelauncher.Launcher' && window.get_title() === 'Lumen Settings')
            .forEach(window => window.delete(global.get_current_time())); true''')
        for _ in range(5): spin()
        print('PASS: normal applications and explicitly opened Settings remain available in the dock')
        group.activate_action('show', None)
        for _ in range(5): spin()
        assert_palette_unlisted()
        assert call('DisableExtension', '(s)', (UUID,))[0]
        for _ in range(5): spin()
        state = palette_state()
        assert state['windows'] and all(not window['skipTaskbar'] for window in state['windows']), state
        assert state['listed'], state
        assert call('EnableExtension', '(s)', (UUID,))[0]
        for _ in range(5): spin()
        assert_palette_unlisted()
        print('PASS: palette stays out of the dock across reopening; disabling and re-enabling restores window-list membership')
        group.activate_action('quit', None)
        deadline = time.monotonic() + 5
        def launcher_running():
            return invoke('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'NameHasOwner', '(s)', ('io.github.gnomelauncher.Launcher',))[0]
        while launcher_running() and time.monotonic() < deadline: spin()
        assert not launcher_running(), 'Launcher did not stop for the cold activation test'
        app.wait(timeout=5)
        deadline = time.monotonic() + 5
        while palette_state()['windows'] and time.monotonic() < deadline: spin()
        assert not palette_state()['windows'], 'Old launcher windows are still managed'
        shell_eval('global._lumenDockTrace.icons.length = 0; global._lumenDockTrace.listedAtSignal.length = 0; true')
        for symbol, pressed in [(0xffeb, True), (0x20, True), (0x20, False), (0xffeb, False)]:
            remote('NotifyKeyboardKeysym', '(ub)', (symbol, pressed))
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            state = palette_state()
            if state['windows'] and any(window['focused'] for window in state['windows']): break
            spin()
        assert_palette_unlisted()
        trace = shell_eval('global._lumenDockTrace')
        assert not trace['icons'] and trace['listedAtSignal'] and not any(event['listed'] for event in trace['listedAtSignal']), trace
        print('PASS: a cold D-Bus activation also creates zero dock icons or transient dock entries')
        remote('Stop')
    finally:
        try:
            if invoke('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'NameHasOwner', '(s)', ('io.github.gnomelauncher.Launcher',))[0]:
                invoke('io.github.gnomelauncher.Launcher', '/io/github/gnomelauncher/Launcher', 'org.gtk.Actions', 'Activate', '(sava{sv})', ('quit', [], {}))
        except GLib.Error:
            pass
        app.terminate(); app.wait(timeout=5); app_log.close()
    assert call('GetExtensionErrors', '(s)', (UUID,))[0] == []
    assert call('DisableExtension', '(s)', (UUID,))[0]
    for _ in range(10): spin()
    for key, before in original.items(): assert list(wm.get_strv(key)) == before, (key, wm.get_strv(key), before)
    print('PASS: GNOME 50 activation, virtual Super+Space, app activation, disable, and shortcut restoration')
finally:
    shell.terminate()
    try: shell.wait(timeout=8)
    except subprocess.TimeoutExpired: shell.kill(); shell.wait()
    log.close()
