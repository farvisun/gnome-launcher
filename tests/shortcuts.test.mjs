import test from 'node:test';
import assert from 'node:assert/strict';
import {canonical, changedBindings, shouldRestore} from '../shell/shortcuts.js';
const schema = 'org.gnome.desktop.wm.keybindings';
test('modifier order does not change shortcut identity', () => {
  assert.equal(canonical('<Shift><Super>space'), canonical('<Super><Shift>space'));
});
test('moves forward layout switch while preserving the hardware key', () => {
  const result = changedBindings(schema, 'switch-input-source', ['<Super>space', 'XF86Keyboard'], '<Super>space');
  assert.deepEqual(result.after, ['<Super><Shift>space', 'XF86Keyboard']);
});
test('removes shifted chord from backward layout switch', () => {
  const result = changedBindings(schema, 'switch-input-source-backward', ['<Shift><Super>space', '<Shift>XF86Keyboard'], '<Super>space');
  assert.deepEqual(result.after, ['<Shift>XF86Keyboard']);
});
test('restores only values owned by the launcher', () => {
  const result = changedBindings(schema, 'switch-input-source', ['<Super>space'], '<Super>space');
  assert.equal(shouldRestore(result.after, result), true);
  assert.equal(shouldRestore(['<Alt>space'], result), false);
});
test('custom shortcut leaves unrelated bindings alone', () => {
  assert.equal(changedBindings(schema, 'switch-input-source', ['<Super>space'], '<Control><Alt>space'), null);
});
