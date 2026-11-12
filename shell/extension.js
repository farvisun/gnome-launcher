import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension, InjectionManager} from 'resource:///org/gnome/shell/extensions/extension.js';
import {changedBindings, shouldRestore} from './shortcuts.js';

const APP = 'io.github.gnomelauncher.Launcher';
const PATH = '/io/github/gnomelauncher/Launcher';
// Open animation, run on the compositor so the GTK client never redraws for it.
const OPEN_MS = 160;
const OPEN_SCALE = 0.965;
const OPEN_LIFT = -8;

export default class Lumen extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._sources = new Set();
        this._hiddenWindows = new Map();
        this._launcherOwner = null;
        this._launcherPid = 0;
        this._injections = new InjectionManager();
        const lumen = this;
        // Mutter initially reports a window-backed app with no app ID or title.
        // Filter by the resident process before the dock can create its icon.
        this._injections.overrideMethod(Shell.AppSystem.prototype, 'get_running', original => function (...args) {
            return original.apply(this, args).filter(app => lumen._showInDock(app));
        });
        // The default map effect zooms a window up from its bottom edge, which
        // reads as a jump for a palette. Skip it; _animateIn plays instead.
        this._injections.overrideMethod(Main.wm, '_shouldAnimateActor', original => function (actor, types) {
            if (lumen._isLauncher(actor?.meta_window)) {
                this._skippedActors?.delete(actor);
                return false;
            }
            return original.call(this, actor, types);
        });
        this._ownerWatch = Gio.bus_watch_name_on_connection(Gio.DBus.session, APP, Gio.BusNameWatcherFlags.NONE,
            (_connection, _name, owner) => {
                if (this._launcherOwner !== owner) {
                    this._launcherOwner = owner;
                    this._launcherPid = 0;
                }
                this._readLauncherPid(owner).catch(error => console.warn(`Lumen process lookup: ${error.message}`));
            }, () => { this._launcherOwner = null; this._launcherPid = 0; });
        this._restore();
        this._bind();
        this._changed = this._settings.connect('changed::toggle-lumen', () => {
            Main.wm.removeKeybinding('toggle-lumen');
            this._restore(); this._bind();
        });
        this._created = global.display.connect('window-created', (_display, window) => this._hideFromDock(window));
        this._map = global.window_manager.connect('map', (_wm, actor) => {
            if (this._isLauncher(actor.meta_window)) {
                this._hideFromDock(actor.meta_window);
                // Center before the first frame so the palette never appears
                // elsewhere and jumps; the idle pass catches a late resize.
                this._position(actor.meta_window);
                this._animateIn(actor);
                this._later(() => this._position(actor.meta_window));
            }
        });
        for (const window of global.display.list_all_windows()) this._hideFromDock(window);
    }
    _animateIn(actor) {
        actor.remove_all_transitions();
        if (!St.Settings.get().enable_animations) return;
        actor.set_pivot_point(0.5, 0.5);
        actor.set({opacity: 0, scale_x: OPEN_SCALE, scale_y: OPEN_SCALE, translation_y: OPEN_LIFT});
        actor.ease({
            opacity: 255, scale_x: 1, scale_y: 1, translation_y: 0,
            duration: OPEN_MS,
            mode: Clutter.AnimationMode.EASE_OUT_CUBIC,
            onStopped: () => {
                actor.set_pivot_point(0, 0);
                actor.set({opacity: 255, scale_x: 1, scale_y: 1, translation_y: 0});
            },
        });
    }
    _isLauncher(window) {
        // GTK's application ID can arrive after the map signal on a live desktop.
        return window && window.get_title() === 'Lumen Launcher' &&
            (window.get_gtk_application_id() === APP || window.get_wm_class() === APP ||
                (this._launcherPid && window.get_pid() === this._launcherPid));
    }
    _showInDock(app) {
        const windows = app.get_windows().filter(window =>
            (this._launcherPid && window.get_pid() === this._launcherPid) ||
            window.get_gtk_application_id() === APP || window.get_wm_class() === APP);
        if (app.get_id() !== `${APP}.desktop` && windows.length === 0) return true;
        return windows.some(window => window.get_title() === 'Lumen Settings' && !window.is_skip_taskbar());
    }
    _busCall(method, parameters) {
        return new Promise((resolve, reject) => {
            Gio.DBus.session.call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', method,
                parameters, null, Gio.DBusCallFlags.NONE, 5000, null, (connection, result) => {
                    try { resolve(connection.call_finish(result).deep_unpack()); }
                    catch (error) { reject(error); }
                });
        });
    }
    async _readLauncherPid(owner) {
        const [pid] = await this._busCall('GetConnectionUnixProcessID', new GLib.Variant('(s)', [owner]));
        if (!this._settings || this._launcherOwner !== owner) return false;
        this._launcherPid = pid;
        return true;
    }
    _hideFromDock(window) {
        if (!this._isLauncher(window) || this._hiddenWindows.has(window)) return;
        const restore = !window.is_skip_taskbar();
        const id = window.connect('unmanaged', () => {
            window.disconnect(id);
            this._hiddenWindows.delete(window);
        });
        this._hiddenWindows.set(window, {id, restore});
        if (restore) window.hide_from_window_list();
    }
    _later(callback) {
        const id = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => { this._sources.delete(id); callback(); return GLib.SOURCE_REMOVE; });
        this._sources.add(id);
    }
    _position(window) {
        // GTK may unmanage a window before its queued positioning idle runs.
        // Calling Mutter's move functions on that old window can crash Shell.
        if (!window || !this._hiddenWindows.has(window) || window.is_hidden()) return;
        const monitor = global.display.get_current_monitor();
        window.move_to_monitor(monitor);
        const area = window.get_work_area_for_monitor(monitor);
        const rect = window.get_frame_rect();
        window.move_frame(true, Math.max(area.x, area.x + Math.round((area.width - rect.width) / 2)), Math.max(area.y, area.y + Math.round((area.height - rect.height) / 3)));
        Main.activateWindow(window);
    }
    _bind() {
        const shortcut = this._settings.get_strv('toggle-lumen')[0];
        if (!shortcut) return;
        const changes = [];
        for (const schema of ['org.gnome.desktop.wm.keybindings', 'org.gnome.shell.keybindings', 'org.gnome.mutter.keybindings', 'org.gnome.mutter.wayland.keybindings']) {
            const source = Gio.SettingsSchemaSource.get_default().lookup(schema, true);
            if (!source) continue;
            const settings = new Gio.Settings({settings_schema: source});
            for (const key of source.list_keys()) {
                if (source.get_key(key).get_value_type().dup_string() !== 'as') continue;
                const change = changedBindings(schema, key, settings.get_strv(key), shortcut);
                if (change) changes.push(change);
            }
        }
        // Persist the journal before changing any desktop preferences.
        this._settings.set_string('shortcut-backup', JSON.stringify(changes)); Gio.Settings.sync();
        for (const change of changes) new Gio.Settings({schema_id: change.schema}).set_strv(change.key, change.after);
        Main.wm.addKeybinding('toggle-lumen', this._settings, Meta.KeyBindingFlags.NONE, Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW, () => this._toggle());
    }
    _restore() {
        const saved = this._settings.get_string('shortcut-backup');
        if (!saved) return;
        try {
            for (const entry of JSON.parse(saved)) {
                const settings = new Gio.Settings({schema_id: entry.schema});
                if (shouldRestore(settings.get_strv(entry.key), entry)) settings.set_strv(entry.key, entry.before);
            }
            this._settings.set_string('shortcut-backup', ''); Gio.Settings.sync();
        } catch (error) { console.error(`Lumen could not restore shortcut journal: ${error}`); }
    }
    async _toggle() {
        if (Main.overview.visible) Main.overview.hide();
        try {
            if (!this._launcherPid) {
                // Start a cold launcher in the background, then identify it before
                // asking GTK to create a visible window.
                await this._busCall('StartServiceByName', new GLib.Variant('(su)', [APP, 0]));
                if (!this._settings) return;
                const [owner] = await this._busCall('GetNameOwner', new GLib.Variant('(s)', [APP]));
                this._launcherOwner = owner;
                if (!await this._readLauncherPid(owner)) return;
            }
        } catch (error) {
            if (this._settings) Main.notifyError('Lumen could not open', error.message);
            return;
        }
        if (!this._settings) return;
        Gio.DBus.session.call(APP, PATH, 'org.gtk.Actions', 'Activate', new GLib.Variant('(sava{sv})', ['toggle', [], {}]), null, Gio.DBusCallFlags.NONE, 5000, null, (connection, result) => {
            if (!this._settings) return;
            try {
                connection.call_finish(result);
                for (const window of global.display.list_all_windows()) {
                    if (this._isLauncher(window)) this._later(() => this._position(window));
                }
            } catch (error) { Main.notifyError('Lumen could not open', `${error.message}. Check that the launcher is installed.`); }
        });
    }
    disable() {
        if (this._changed) this._settings.disconnect(this._changed);
        if (this._ownerWatch) Gio.bus_unwatch_name(this._ownerWatch);
        if (this._created) global.display.disconnect(this._created);
        if (this._map) global.window_manager.disconnect(this._map);
        for (const id of this._sources) GLib.Source.remove(id);
        this._sources.clear();
        for (const [window, {id, restore}] of this._hiddenWindows) {
            window.disconnect(id);
            if (restore) window.show_in_window_list();
        }
        this._hiddenWindows.clear();
        this._injections.clear();
        this._launcherOwner = null; this._launcherPid = 0;
        Main.wm.removeKeybinding('toggle-lumen'); this._restore(); this._settings = null;
    }
}
