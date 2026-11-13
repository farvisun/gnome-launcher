import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {clockTime, menuModel, readPrefs, readWeather, weatherIcon, weatherUrl} from '../shell/weather-model.js';

const text = readFileSync(new URL('./fixtures/wttr-berlin.json', import.meta.url), 'utf8');

test('reads the Weather preferences with Raycast defaults', () => {
  const off = readPrefs('{}');
  assert.equal(off.enabled, false);
  assert.equal(off.clock, '12h');
  assert.equal(off.showIcon && off.showText, true);
  const on = readPrefs(JSON.stringify({enabled_builtins: ['lumen.weather'],
    preferences: {'lumen.weather': {defaultquery: ' Paris ', unitsystem: 'imperial', menutemperature: 'feelslike', showmenuicon: false}}}));
  assert.deepEqual([on.enabled, on.location, on.imperial, on.feelsLike, on.showIcon], [true, 'Paris', true, true, false]);
  assert.equal(weatherUrl('New York'), 'https://wttr.in/New%20York?format=j1');
  assert.equal(readPrefs('not json').enabled, false);
});

test('builds the panel label and Raycast menu sections', () => {
  const data = readWeather(text);
  assert.ok(data);
  assert.equal(readWeather(JSON.stringify({data})).weather.length, 3);
  assert.equal(readWeather('{"weather":[]}'), null);
  const prefs = {...readPrefs('{}'), clock: '24h'};
  const model = menuModel(data, prefs, Date.now() - 5 * 60_000);
  assert.deepEqual(model.panel, {icon: 'weather-clear-symbolic', text: '8 °C', tooltip: 'Sunny · Berlin, Berlin, Germany'});
  assert.deepEqual(model.sections.map(s => s.title), ['Weather', 'Temperature', 'Air', 'Sun', 'Moon', 'Forecast', 'Location', null]);
  const forecast = model.sections.find(s => s.title === 'Forecast').items;
  assert.equal(forecast.length, 3);
  assert.equal(forecast[0].day, data.weather[0].date);
  assert.match(forecast[0].value, /^⬆ \d+ °C ⬇ \d+ °C/);
  const sun = model.sections.find(s => s.title === 'Sun').items;
  assert.equal(sun.find(r => r.title === 'Sunset').value, '16:16');
  assert.equal(model.sections.at(-1).items.find(r => r.title === 'Fetched').value, '5 minutes ago');
  const feels = menuModel(data, {...prefs, feelsLike: true, imperial: true, showIcon: false}, 0);
  assert.deepEqual([feels.panel.icon, feels.panel.text], [null, '39 °F']);
  const neither = menuModel(data, {...prefs, showIcon: false, showText: false}, 0);
  assert.deepEqual([neither.panel.icon, neither.panel.text], ['weather-overcast-symbolic', '']);
});

test('maps codes and clocks', () => {
  assert.equal(weatherIcon('113'), 'weather-clear-symbolic');
  assert.equal(weatherIcon('395'), 'weather-snow-symbolic');
  assert.equal(clockTime('04:16 PM', '24h'), '16:16');
  assert.equal(clockTime('04:16 PM', '12h'), '04:16 PM');
});
