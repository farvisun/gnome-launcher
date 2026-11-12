// Pure helpers shared with Node tests. Never overwrite a user's later edits.
export function canonical(shortcut) {
    const modifiers = [...shortcut.toLowerCase().matchAll(/<([^>]+)>/g)].map(m => ({primary: 'control', ctrl: 'control', meta: 'super'}[m[1]] ?? m[1])).sort();
    const key = shortcut.replace(/<[^>]+>/g, '').toLowerCase();
    return `${modifiers.join('+')}:${key}`;
}
export function isSame(a, b) { return canonical(a) === canonical(b); }
export function changedBindings(schema, key, before, shortcut) {
    let after = before.filter(value => !isSame(value, shortcut));
    if (schema === 'org.gnome.desktop.wm.keybindings' && isSame(shortcut, '<Super>space')) {
        if (key === 'switch-input-source') {
            after = before.filter(value => !isSame(value, '<Super>space'));
            if (!after.some(value => isSame(value, '<Super><Shift>space'))) after.unshift('<Super><Shift>space');
        } else if (key === 'switch-input-source-backward') {
            after = before.filter(value => !isSame(value, '<Super><Shift>space'));
        }
    }
    return JSON.stringify(before) === JSON.stringify(after) ? null : {schema, key, before, after};
}
export function shouldRestore(current, entry) { return JSON.stringify(current) === JSON.stringify(entry.after); }
