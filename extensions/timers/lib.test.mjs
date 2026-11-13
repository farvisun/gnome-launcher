import test from 'node:test';
import assert from 'node:assert/strict';
import {elapsed, escapeMarkup, formatClock, formatDuration, fractionDone, normalize, parseDuration, timeLeft} from './lib.mjs';

test('parses durations with optional names in either order', () => {
  assert.deepEqual(parseDuration('5'), {seconds: 300, name: ''});
  assert.deepEqual(parseDuration('90s'), {seconds: 90, name: ''});
  assert.deepEqual(parseDuration('1h30m'), {seconds: 5400, name: ''});
  assert.deepEqual(parseDuration('1h 30m'), {seconds: 5400, name: ''});
  assert.deepEqual(parseDuration('1:30'), {seconds: 90, name: ''});
  assert.deepEqual(parseDuration('1:00:00'), {seconds: 3600, name: ''});
  assert.deepEqual(parseDuration('25m tea'), {seconds: 1500, name: 'tea'});
  assert.deepEqual(parseDuration('green tea 4 min'), {seconds: 240, name: 'green tea'});
  assert.deepEqual(parseDuration('2.5m'), {seconds: 150, name: ''});
});
test('rejects text without a usable duration', () => {
  for (const text of ['', 'tea', '0', '1:75', '100h', 'm5']) assert.equal(parseDuration(text), null, text);
});
test('formats durations and clocks', () => {
  assert.equal(formatDuration(5400), '1 h 30 min');
  assert.equal(formatDuration(150), '2 min 30 s');
  assert.equal(formatClock(65_000), '1:05');
  assert.equal(formatClock(3_723_000), '1:02:03');
  assert.equal(formatClock(-5), '0:00');
});
test('drops rung timers, sorts the rest and measures stopwatches', () => {
  const state = normalize({timers: [{id: 'b', endsAt: 30}, {id: 'old', endsAt: 5}, {id: 'a', endsAt: 20}], presets: 'bad'}, 10);
  assert.deepEqual(state.timers.map(t => t.id), ['a', 'b']);
  assert.deepEqual(state.presets, []);
  assert.equal(elapsed({accumulatedMs: 1000, startedAt: 5000}, 7000), 3000);
  assert.equal(elapsed({accumulatedMs: 1000, startedAt: null}, 7000), 1000);
  assert.equal(escapeMarkup('a<b>&'), 'a&lt;b&gt;&amp;');
});
test('keeps paused timers after running ones and measures progress', () => {
  const state = normalize({timers: [{id: 'p', endsAt: 1, pausedLeftMs: 5000, seconds: 10}, {id: 'r', endsAt: 20_000, seconds: 20}]}, 10_000);
  assert.deepEqual(state.timers.map(t => t.id), ['r', 'p']);
  assert.equal(timeLeft(state.timers[1], 10_000), 5000);
  assert.equal(fractionDone(state.timers[0], 10_000), 0.5);
  assert.equal(fractionDone(state.timers[1], 10_000), 0.5);
});
