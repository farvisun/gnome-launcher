import test from 'node:test';
import assert from 'node:assert/strict';
import {formatClock, panelText, readTimers, timersEnabled} from '../shell/timers-model.js';

const storage = JSON.stringify({state: {
  timers: [{id: 'b', name: 'Tea', endsAt: 70_000}, {id: 'a', name: 'Pasta', endsAt: 40_000}, {id: 'c', name: 'Rung', endsAt: 5_000}],
  stopwatches: [{id: 's', name: 'Focus', startedAt: 1_000, accumulatedMs: 500}],
}});

test('reads running timers soonest first and drops rung ones', () => {
  const model = readTimers(storage, 10_000);
  assert.deepEqual(model.timers.map(t => t.name), ['Pasta', 'Tea']);
  assert.equal(model.active, true);
  assert.equal(panelText(model, 10_000), '0:30');
});
test('falls back to a running stopwatch, and stays hidden when paused or broken', () => {
  const watch = readTimers(JSON.stringify({state: {stopwatches: [{id: 's', name: 'Focus', startedAt: 1_000, accumulatedMs: 500}]}}), 61_000);
  assert.equal(panelText(watch, 61_000), '1:01');
  const paused = readTimers(JSON.stringify({state: {stopwatches: [{id: 's', startedAt: null, accumulatedMs: 5}]}}), 0);
  assert.equal(paused.active, false);
  assert.equal(readTimers('not json', 0).active, false);
});
test('lists paused timers without counting them down', () => {
  const model = readTimers(JSON.stringify({state: {timers: [{id: 'p', name: 'Bread', endsAt: 1, pausedLeftMs: 90_000}]}}), 10_000);
  assert.deepEqual(model.paused, [{name: 'Bread', leftMs: 90_000}]);
  assert.equal(model.timers.length, 0);
  assert.equal(model.active, false);
});
test('needs the Timers extension turned on', () => {
  assert.equal(timersEnabled(JSON.stringify({enabled_builtins: ['lumen.timers']})), true);
  assert.equal(timersEnabled(JSON.stringify({disabled: []})), false);
  assert.equal(timersEnabled(''), false);
  assert.equal(formatClock(3_723_000), '1:02:03');
});
