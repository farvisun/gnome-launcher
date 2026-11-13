import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {elapsed, formatClock, panelText, readTimers, timersEnabled} from './timers-model.js';

const TimerButton = GObject.registerClass(
class TimerButton extends PanelMenu.Button {
    _init(openCommand) {
        super._init(0.5, 'Lumen Timers');
        this._openCommand = openCommand;
        this._rows = [];
        const box = new St.BoxLayout({style_class: 'panel-status-menu-box'});
        box.add_child(new St.Icon({icon_name: 'alarm-symbolic', style_class: 'system-status-icon'}));
        this._label = new St.Label({y_align: Clutter.ActorAlign.CENTER});
        box.add_child(this._label);
        this.add_child(box);
    }
    // Not `show`: that would shadow Clutter.Actor.show().
    present(model, now) {
        this._rows = [];
        this.menu.removeAll();
        for (const timer of model.timers) {
            this._addRow(timer.name, at => `${formatClock(timer.endsAt - at)} left`);
        }
        for (const timer of model.paused) {
            this._addRow(timer.name, () => `${formatClock(timer.leftMs)} left · paused`);
        }
        for (const watch of model.stopwatches) {
            this._addRow(watch.name, at => `${formatClock(elapsed(watch, at))}${watch.startedAt === null ? ' · paused' : ''}`);
        }
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this.menu.addAction('Manage Timers…', () => this._openCommand('lumen.timers/manage-timers'));
        this.menu.addAction('Manage Stopwatches…', () => this._openCommand('lumen.timers/manage-stopwatches'));
        this.tick(model, now);
    }
    _addRow(name, value) {
        const item = new PopupMenu.PopupMenuItem(name, {reactive: false, can_focus: false});
        const label = new St.Label({x_expand: true, x_align: Clutter.ActorAlign.END, style_class: 'popup-inactive-menu-item'});
        item.add_child(label);
        this.menu.addMenuItem(item);
        this._rows.push(at => label.set_text(value(at)));
    }
    tick(model, now) {
        this._label.set_text(panelText(model, now));
        for (const update of this._rows) update(now);
    }
});

/**
 * Shows the next Lumen timer in the top bar. It reads the Timers extension's
 * storage file and Lumen's state.json, so it needs no running launcher.
 */
export class TimerTray {
    constructor(settings, openCommand) {
        this._settings = settings;
        this._openCommand = openCommand;
        const data = GLib.build_filenamev([GLib.get_user_data_dir(), 'gnome-launcher']);
        this._storage = Gio.File.new_for_path(GLib.build_filenamev([data, 'extension-storage', 'lumen.timers.json']));
        this._state = Gio.File.new_for_path(GLib.build_filenamev([data, 'state.json']));
        // Lumen replaces both files by renaming a temporary file over them.
        this._monitors = [this._storage, this._state].map(file => {
            const monitor = file.monitor_file(Gio.FileMonitorFlags.WATCH_MOVES, null);
            monitor.connect('changed', () => this._queueReload());
            return monitor;
        });
        this._changed = settings.connect('changed::show-timer-indicator', () => this._reload());
        this._reload();
    }
    _read(file) {
        try {
            const [, bytes] = file.load_contents(null);
            return new TextDecoder().decode(bytes);
        } catch {
            return '';
        }
    }
    _queueReload() {
        if (this._pending) return;
        this._pending = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 100, () => {
            this._pending = 0;
            this._reload();
            return GLib.SOURCE_REMOVE;
        });
    }
    _reload() {
        const now = Date.now();
        this._model = readTimers(this._read(this._storage), now);
        const visible = this._settings.get_boolean('show-timer-indicator') &&
            this._model.active && timersEnabled(this._read(this._state));
        if (!visible) {
            this._hide();
            return;
        }
        if (!this._button) {
            this._button = new TimerButton(this._openCommand);
            Main.panel.addToStatusArea('lumen-timers', this._button, 0, 'right');
        }
        this._button.present(this._model, now);
        this._tickId ??= GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 1, () => {
            const at = Date.now();
            // A timer that just rang leaves the list; reload to drop or hide it.
            if (this._model.timers.some(t => t.endsAt <= at)) this._reload();
            else this._button?.tick(this._model, at);
            return this._button ? GLib.SOURCE_CONTINUE : GLib.SOURCE_REMOVE;
        });
    }
    _hide() {
        if (this._tickId) GLib.Source.remove(this._tickId);
        this._tickId = null;
        this._button?.destroy();
        this._button = null;
    }
    destroy() {
        this._hide();
        if (this._pending) GLib.Source.remove(this._pending);
        this._pending = 0;
        for (const monitor of this._monitors) monitor.cancel();
        this._monitors = [];
        this._settings.disconnect(this._changed);
    }
}
