import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Soup from 'gi://Soup?version=3.0';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {REFRESH_MS, WEATHER_ID, menuModel, readPrefs, readWeather, weatherUrl} from './weather-model.js';

const WeatherButton = GObject.registerClass(
class WeatherButton extends PanelMenu.Button {
    _init(actions) {
        super._init(0.5, 'Lumen Weather');
        this._actions = actions;
        const box = new St.BoxLayout({style_class: 'panel-status-menu-box'});
        this._icon = new St.Icon({icon_name: 'weather-overcast-symbolic', style_class: 'system-status-icon'});
        box.add_child(this._icon);
        this._label = new St.Label({y_align: Clutter.ActorAlign.CENTER});
        box.add_child(this._label);
        this.add_child(box);
    }
    present(model) {
        this._icon.visible = Boolean(model.panel.icon);
        if (model.panel.icon) this._icon.icon_name = model.panel.icon;
        this._label.text = model.panel.text;
        this._label.visible = Boolean(model.panel.text);
        this.menu.removeAll();
        model.sections.forEach((section, index) => {
            if (index > 0) this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem(section.title ?? undefined));
            else if (section.title) this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem(section.title));
            for (const row of section.items) this._addRow(row);
        });
    }
    showError(message) {
        this._icon.visible = true;
        this._icon.icon_name = 'weather-severe-alert-symbolic';
        this._label.visible = false;
        this.menu.removeAll();
        this.menu.addMenuItem(new PopupMenu.PopupMenuItem(`Error: ${message}`, {reactive: false}));
        this.menu.addAction('Show Weather…', () => this._actions.open(null));
    }
    _addRow(row) {
        const item = row.icon
            ? new PopupMenu.PopupImageMenuItem(row.title, row.icon)
            : new PopupMenu.PopupMenuItem(row.title);
        if (row.value) {
            const value = new St.Label({text: row.value, x_expand: true, x_align: Clutter.ActorAlign.END, style_class: 'popup-inactive-menu-item'});
            item.add_child(value);
        }
        if (row.tooltip) item.accessible_name = `${row.title}: ${row.value}. ${row.tooltip}`;
        item.connect('activate', () => {
            if (row.url) this._actions.url(row.url);
            else if (row.settings) this._actions.settings();
            else this._actions.open(row.day ?? null);
        });
        this.menu.addMenuItem(item);
    }
});

/**
 * Shows the weather in the top bar, like Raycast's Weather Menu Bar. It asks
 * wttr.in itself every 30 minutes, with the Weather extension's preferences
 * from Lumen's state.json, so Lumen does not need to be open.
 */
export class WeatherTray {
    constructor(settings, actions) {
        this._settings = settings;
        this._actions = actions;
        this._session = new Soup.Session({timeout: 30, user_agent: 'curl/8 (Lumen weather)'});
        this._cancellable = null;
        this._data = null;
        this._fetchedAt = 0;
        this._fetchedFor = null;
        const data = GLib.build_filenamev([GLib.get_user_data_dir(), 'gnome-launcher']);
        this._state = Gio.File.new_for_path(GLib.build_filenamev([data, 'state.json']));
        // Lumen replaces state.json by renaming a temporary file over it.
        this._monitor = this._state.monitor_file(Gio.FileMonitorFlags.WATCH_MOVES, null);
        this._monitor.connect('changed', () => this._queueReload());
        this._changed = settings.connect('changed::show-weather-indicator', () => this._reload());
        this._reload();
    }
    _read() {
        try {
            const [, bytes] = this._state.load_contents(null);
            return new TextDecoder().decode(bytes);
        } catch {
            return '';
        }
    }
    _queueReload() {
        if (this._pending) return;
        this._pending = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 200, () => {
            this._pending = 0;
            this._reload();
            return GLib.SOURCE_REMOVE;
        });
    }
    _reload() {
        this._prefs = readPrefs(this._read());
        const visible = this._settings.get_boolean('show-weather-indicator') && this._prefs.enabled;
        if (!visible) {
            this._hide();
            return;
        }
        if (!this._button) {
            this._button = new WeatherButton(this._actions);
            Main.panel.addToStatusArea('lumen-weather', this._button, 1, 'right');
            this._refreshId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, REFRESH_MS, () => {
                this._fetch();
                return GLib.SOURCE_CONTINUE;
            });
        }
        const key = `${this._prefs.location}|${this._prefs.imperial}`;
        if (this._data && this._fetchedFor === this._prefs.location) this._present();
        if (!this._data || this._fetchedFor !== this._prefs.location || this._lastKey !== key) this._fetch();
        this._lastKey = key;
    }
    _present() {
        if (!this._button || !this._data) return;
        try {
            this._button.present(menuModel(this._data, this._prefs, this._fetchedAt));
        } catch (error) {
            this._button.showError(error.message);
        }
    }
    _fetch() {
        this._cancellable?.cancel();
        const cancellable = new Gio.Cancellable();
        this._cancellable = cancellable;
        const location = this._prefs.location;
        const message = Soup.Message.new('GET', weatherUrl(location));
        message.request_headers.append('Accept', 'application/json');
        this._session.send_and_read_async(message, GLib.PRIORITY_DEFAULT, cancellable, (session, result) => {
            if (cancellable.is_cancelled()) return;
            try {
                const bytes = session.send_and_read_finish(result);
                const data = readWeather(new TextDecoder().decode(bytes.get_data()));
                if (!data) throw new Error(`wttr.in answered ${message.status_code} without weather data`);
                this._data = data;
                this._fetchedAt = Date.now();
                this._fetchedFor = location;
                this._present();
            } catch (error) {
                // Keep showing the last report; only an empty indicator shows the error.
                if (this._data) this._present();
                else this._button?.showError(error.message);
            }
        });
    }
    _hide() {
        this._cancellable?.cancel();
        this._cancellable = null;
        if (this._refreshId) GLib.Source.remove(this._refreshId);
        this._refreshId = 0;
        this._button?.destroy();
        this._button = null;
    }
    destroy() {
        this._hide();
        if (this._pending) GLib.Source.remove(this._pending);
        this._pending = 0;
        this._monitor?.cancel();
        this._monitor = null;
        this._settings.disconnect(this._changed);
        this._session?.abort();
        this._session = null;
    }
}

export {WEATHER_ID};
