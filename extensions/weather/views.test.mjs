import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {dayView, errorView, weatherView} from './views.mjs';

const data = JSON.parse(readFileSync(new URL('../../tests/fixtures/wttr-berlin.json', import.meta.url)));

test('the report lists current conditions, then each forecast day', () => {
  const view = weatherView(data, {unitsystem: 'metric', clockformat: '24h'}, Date.UTC(2026, 10, 13, 13, 6));
  const ids = view.items.map(i => i.id);
  assert.deepEqual(ids.slice(0, 8), ['current', 'feels-like', 'uv-index', 'pressure', 'visibility', 'coordinates', 'sun', 'moon']);
  assert.equal(view.items[0].section, 'Weather Report • Berlin, Berlin, Germany • 30 minutes ago');
  assert.equal(view.items.filter(i => i.section === 'Daily Forecast').length, 3);
  const sun = view.items.find(i => i.id === 'sun');
  assert.deepEqual(sun.accessories.map(a => a.text), ['07:25', '16:16']);
  assert.ok(view.items.every(i => (i.accessories ?? []).length <= 6));
  assert.equal(view.items.find(i => i.id.startsWith('day:')).actions[0].title, 'Show Details');
});

test('a day lists its hours and keeps its context for re-queries', () => {
  const date = data.weather[1].date;
  const view = dayView(data, date, {unitsystem: 'imperial', clockformat: '12h'}, 'Berlin');
  assert.equal(view.items.length, 8);
  assert.equal(view.items[0].title, '12 AM');
  assert.match(view.items[0].subtitle, /°F {5}\S/);
  assert.deepEqual(view.context, {day: date, location: 'Berlin'});
  assert.equal(dayView(data, '1999-01-01').empty.title, 'Could not fetch data from wttr.in');
  assert.equal(errorView('boom').empty.description, 'boom');
});
