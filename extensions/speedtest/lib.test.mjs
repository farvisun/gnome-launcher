import test from 'node:test';
import assert from 'node:assert/strict';
import {ACTIVITIES, activityLevels, bitsPerSecond, formatBytes, formatMs, formatSpeed, gauge, jitter, median, percentile, pingRating, serverTime, speedParts, speedRating, summary} from './lib.mjs';

test('computes latency statistics', () => {
  assert.equal(median([5, 1, 3]), 3);
  assert.equal(median([4, 1, 3, 2]), 2.5);
  assert.equal(median([]), null);
  assert.equal(jitter([10, 12, 9, 9]), (2 + 3 + 0) / 3);
  assert.equal(jitter([10]), null);
  assert.equal(percentile([1, 2, 3, 4, 5], 0.9), 4.6);
});
test('reads the longest Server-Timing duration', () => {
  assert.equal(serverTime('cfSpeedEdge;dur=3, cfSpeedWorker;dur=22'), 22);
  assert.equal(serverTime('cfRequestDuration;dur=12.5'), 12.5);
  assert.equal(serverTime(null), 0);
});
test('converts and formats speeds', () => {
  assert.equal(bitsPerSecond(1_000_000, 1000), 8_000_000);
  assert.equal(bitsPerSecond(1_000_000, 5), null);
  assert.equal(formatSpeed(8_000_000), '8.0 Mbps');
  assert.equal(formatSpeed(1.5e9), '1.50 Gbps');
  assert.equal(formatSpeed(640_000), '640 kbps');
  assert.equal(formatMs(4.26), '4.3 ms');
  assert.equal(formatMs(23.6), '24 ms');
  assert.match(summary({download: 1e8, upload: null, ping: 12, jitter: 1, server: 'Frankfurt · FRA'}), /Download: 100.0 Mbps\nUpload: —\nPing: 12 ms/);
});
test('grades a connection for everyday activities', () => {
  const video = ACTIVITIES.find(a => a.id === 'video');
  assert.deepEqual(activityLevels(video, 8e6, 4e6).map(l => l.ok), [true, true, false, false]);
  const stream = ACTIVITIES.find(a => a.id === 'stream');
  assert.deepEqual(activityLevels(stream, 30e6, 0).map(l => l.ok), [true, true, true, true]);
  assert.equal(speedRating(150e6).label, 'Very fast');
  assert.equal(speedRating(2e6).color, 'red');
  assert.equal(pingRating(12, 2).label, 'Excellent');
  assert.equal(pingRating(80, 40).label, 'Poor');
  assert.equal(pingRating(null), null);
  assert.equal(gauge(1e9), 1);
  assert.equal(gauge(0), 0);
  assert.deepEqual(speedParts(245.3e6), {value: '245.3', unit: 'Mbps'});
  assert.equal(formatBytes(25_000_000), '25.0 MB');
});
