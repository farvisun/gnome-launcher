import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const PATH = '/io/github/gnomelauncher/Shell';
const IFACE = `<node>
  <interface name="io.github.gnomelauncher.Shell">
    <method name="ShowHud">
      <arg type="s" direction="in" name="text"/>
      <arg type="s" direction="in" name="icon"/>
    </method>
  </interface>
</node>`;
const VISIBLE_MS = 1800;
const FADE_MS = 160;

/**
 * Raycast's HUD: a short pill near the bottom of the screen, shown after Lumen
 * closes (for example "Copied Password"). Lumen calls ShowHud on the Shell's
 * own bus name, so nothing else needs to run.
 */
export class Hud {
    constructor() {
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, {
            ShowHud: (text, icon) => this.show(text, icon),
        });
        this._dbus.export(Gio.DBus.session, PATH);
    }
    show(text, icon) {
        text = String(text ?? '').replace(/\s+/g, ' ').trim().slice(0, 200);
        if (!text) return;
        this._clear();
        const box = new St.BoxLayout({
            style: 'background-color: rgba(30, 30, 30, 0.92); color: #f6f6f6; border-radius: 999px; '
                + 'padding: 10px 18px; spacing: 8px; font-weight: 600; '
                + 'box-shadow: 0 6px 18px rgba(0, 0, 0, 0.35);',
            opacity: 0,
        });
        if (icon) box.add_child(new St.Icon({icon_name: icon, icon_size: 16, y_align: Clutter.ActorAlign.CENTER}));
        box.add_child(new St.Label({text, y_align: Clutter.ActorAlign.CENTER}));
        Main.uiGroup.add_child(box);
        const monitor = Main.layoutManager.primaryMonitor;
        const [, width] = box.get_preferred_width(-1);
        const [, height] = box.get_preferred_height(width);
        box.set_position(
            Math.round(monitor.x + (monitor.width - width) / 2),
            Math.round(monitor.y + monitor.height - height - monitor.height * 0.12));
        this._box = box;
        box.ease({opacity: 255, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        this._timeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, VISIBLE_MS, () => {
            this._timeout = 0;
            box.ease({
                opacity: 0, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_IN_QUAD,
                onStopped: () => { if (this._box === box) this._clear(); else box.destroy(); },
            });
            return GLib.SOURCE_REMOVE;
        });
    }
    _clear() {
        if (this._timeout) GLib.Source.remove(this._timeout);
        this._timeout = 0;
        this._box?.destroy();
        this._box = null;
    }
    destroy() {
        this._clear();
        this._dbus?.unexport();
        this._dbus = null;
    }
}
