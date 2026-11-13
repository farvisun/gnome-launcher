import test from 'node:test';
import assert from 'node:assert/strict';
import {blank, flat, view} from './views.mjs';

test('shows the five measurements while running and grades the finished test', () => {
  const running = view({...blank(), stage: 'download', progress: {latency: 1, download: 0.4, upload: 0}, download: 80e6, downloadSamples: [70e6, 90e6]});
  assert.deepEqual(running.items.map(i => i.id), ['download', 'upload', 'ping', 'server', 'network']);
  assert.equal(running.items[0].progress, 0.4);
  assert.match(running.items[0].subtitle, /Measuring… 40%/);
  assert.equal(running.detail, true);
  const done = view({...blank(), stage: null, done: true, at: 0, download: 80e6, upload: 20e6, ping: 12, jitter: 2, latencies: [11, 13]});
  assert.ok(done.items.some(i => i.id === 'activity:video'));
  assert.equal(done.items.at(-1).title, 'Fast connection');
  assert.equal(flat({...blank(), download: 1, meta: {isp: 'ISP', city: 'Town'}}).network, 'ISP · Town');
});
