import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {
  areaTitle, clockTime, dayCode, formatRequestQuery, hourLabel, isValid, observedAt, relativeTime,
  temperature, units, weatherIcon, weekday, windArrow, windText,
} from './lib.mjs';

const data = JSON.parse(readFileSync(new URL('../../tests/fixtures/wttr-berlin.json', import.meta.url)));
const metric = units({unitsystem: 'metric', clockformat: '24h'});
const imperial = units({unitsystem: 'imperial', clockformat: '12h'});

test('reads wttr.in conditions in both unit systems', () => {
  assert.ok(isValid(data));
  assert.ok(!isValid({weather: []}));
  assert.equal(areaTitle(data), 'Berlin, Berlin, Germany');
  assert.equal(temperature(data.current_condition[0], metric), '8 °C');
  assert.equal(temperature(data.current_condition[0], imperial), '46 °F');
  assert.equal(windText(data.current_condition[0], metric), '24 km/h → (W)');
  assert.equal(windText(data.current_condition[0], imperial), '15 mph → (W)');
  assert.equal(formatRequestQuery('Lat -33.87 and Lon 151.21'), '33.87°S, 151.21°E');
});

test('maps codes, wind and days like Raycast', () => {
  assert.equal(weatherIcon('113'), 'weather-clear-symbolic');
  assert.equal(weatherIcon('389'), 'weather-storm-symbolic');
  assert.equal(windArrow('0'), '↓');
  assert.equal(windArrow('90'), '←');
  assert.equal(dayCode({hourly: [{weatherCode: '113'}, {weatherCode: '116'}, {weatherCode: '116'}]}), '116');
  assert.equal(weekday('2026-11-13'), 'Friday');
});

test('formats times in the chosen clock', () => {
  assert.equal(clockTime('07:25 AM', metric), '07:25');
  assert.equal(clockTime('04:16 PM', metric), '16:16');
  assert.equal(clockTime('12:05 AM', metric), '00:05');
  assert.equal(clockTime('04:16 PM', imperial), '04:16 PM');
  assert.equal(hourLabel('0', imperial), '12 AM');
  assert.equal(hourLabel('1500', imperial), '3 PM');
  assert.equal(hourLabel('900', metric), '09:00');
});

test('dates the observation and says how long ago it was', () => {
  const now = Date.UTC(2026, 10, 13, 13, 6);
  assert.equal(observedAt({observation_time: '12:36 PM'}, now), Date.UTC(2026, 10, 13, 12, 36));
  // Just after midnight UTC, an observation from 11 PM was yesterday.
  assert.equal(observedAt({observation_time: '11:00 PM'}, Date.UTC(2026, 10, 14, 0, 10)), Date.UTC(2026, 10, 13, 23, 0));
  assert.equal(relativeTime(now - 30 * 60_000, now), '30 minutes ago');
  assert.equal(relativeTime(now - 20_000, now), 'just now');
  assert.equal(relativeTime(now - 3 * 3600_000, now), '3 hours ago');
});
