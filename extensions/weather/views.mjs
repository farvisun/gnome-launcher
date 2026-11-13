// Builds the Weather list views from wttr.in data, as Raycast's weather.tsx
// and day.tsx do. Pure, so the views can be tested and previewed.
import {list} from '../../sdk/index.mjs';
import {
  Icons, areaTitle, areaValues, clockTime, cloudCover, current, dayCode, dayTemperature, feelsLike,
  hourLabel, hourlyRain, hourlyThunder, moon, observedAt, pressure, rain, relativeTime, snow, sun,
  sunHours, temperature, units, visibility, weatherIcon, weekday, windArrow, windText,
} from './lib.mjs';

export const PLACEHOLDER = 'Search for other location (e.g. London)';
const SHOW_DETAILS = [{id: 'primary', title: 'Show Details'}];

/** An accessory, or nothing when there is no value to show. */
const accessory = (icon, text, tooltip) => (text ? {icon, text, tooltip: tooltip ?? text} : null);
const compact = items => items.filter(Boolean);

function reportItems(data, prefs, now) {
  const u = units(prefs);
  const conditions = current(data);
  const description = conditions?.weatherDesc?.[0]?.value?.trim();
  const observed = relativeTime(observedAt(conditions, now), now);
  const section = `Weather Report • ${areaTitle(data)}${observed ? ` • ${observed}` : ''}`;
  const wet = rain(conditions, u);
  const sunTimes = sun(data);
  const moonTimes = moon(data);
  const place = areaValues(data);
  const coords = place?.latitude && place?.longitude ? `${place.longitude}, ${place.latitude}` : undefined;
  const feels = feelsLike(conditions, u);
  const items = [
    {
      id: 'current', section, title: temperature(conditions, u), subtitle: description,
      icon: weatherIcon(conditions?.weatherCode),
      accessories: compact([
        accessory(Icons.Sunrise, sunHours(data), sunHours(data) && `Sun Hours: ${sunHours(data)}`),
        accessory(Icons.Cloud, cloudCover(conditions), cloudCover(conditions) && `Cloud Cover ${cloudCover(conditions)}`),
        wet && wet.value > 0 ? accessory(Icons.Rain, wet.text, `Rain ${wet.text}`) : null,
        accessory(Icons.Humidity, conditions ? `${conditions.humidity}%` : '?', conditions ? `Humidity: ${conditions.humidity}%` : '?'),
        accessory(Icons.Wind, windText(conditions, u), `Wind ${windText(conditions, u)}`),
      ]),
    },
    feels && {id: 'feels-like', section, title: 'Feels Like', icon: Icons.FeelsLike, accessories: [{text: feels}]},
    conditions?.uvIndex && {id: 'uv-index', section, title: 'UV Index', icon: Icons.UVIndex, accessories: [{text: conditions.uvIndex}]},
    pressure(conditions, u) && {id: 'pressure', section, title: 'Pressure', icon: Icons.Pressure, accessories: [{text: pressure(conditions, u)}]},
    visibility(conditions, u) && {id: 'visibility', section, title: 'Visibility', icon: Icons.Visibility, accessories: [{text: visibility(conditions, u)}]},
    coords && {id: 'coordinates', section, title: 'Coordinates', icon: Icons.Coordinates,
      accessories: [{text: coords, tooltip: `Location (Longitude, Latitude): ${coords}`}]},
    sunTimes && {id: 'sun', section, title: 'Sun', icon: Icons.Sun, accessories: [
      {icon: Icons.Sunrise, text: clockTime(sunTimes.sunrise, u), tooltip: `Sunrise ${clockTime(sunTimes.sunrise, u)}`},
      {icon: Icons.Sunset, text: clockTime(sunTimes.sunset, u), tooltip: `Sunset ${clockTime(sunTimes.sunset, u)}`},
    ]},
    moonTimes && {id: 'moon', section, title: 'Moon', subtitle: moonTimes.phase, icon: Icons.Moon, accessories: [
      {icon: Icons.Moonrise, text: clockTime(moonTimes.moonrise, u), tooltip: `Moonrise ${clockTime(moonTimes.moonrise, u)}`, color: 'dim'},
      {icon: Icons.Moonset, text: clockTime(moonTimes.moonset, u), tooltip: `Moonset ${moonTimes.moonset}`, color: 'dim'},
    ]},
  ];
  return compact(items);
}

function dayItems(data, prefs) {
  const u = units(prefs);
  return (data?.weather ?? []).map(day => {
    const max = dayTemperature(day, 'max', u);
    const min = dayTemperature(day, 'min', u);
    const snowfall = snow(day);
    return {
      id: `day:${day.date}`, section: 'Daily Forecast', title: weekday(day.date),
      icon: weatherIcon(dayCode(day)),
      accessories: compact([
        day.sunHour ? {icon: Icons.Sun, text: `${day.sunHour} h`, tooltip: `Sun Hours: ${day.sunHour} h`, color: 'dim'} : null,
        snowfall && snowfall.value > 0 ? {icon: Icons.Snow, text: snowfall.text, tooltip: `Snow: ${snowfall.text}`} : null,
        {icon: Icons.ArrowUp, text: max, tooltip: `Max. Temperature ${max}`},
        {icon: Icons.ArrowDown, text: min, tooltip: `Min. Temperature ${min}`},
      ]),
      actions: SHOW_DETAILS,
    };
  });
}

/** The report for one place: current conditions, then the daily forecast. */
export function weatherView(data, prefs = {}, now = Date.now()) {
  return list([...reportItems(data, prefs, now), ...dayItems(data, prefs)], {placeholder: PLACEHOLDER});
}

/** One day's hourly forecast, pushed over the report for `location`. */
export function dayView(data, date, prefs = {}, location = '') {
  const u = units(prefs);
  const day = data?.weather?.find(d => d.date === date);
  if (!day) return errorView('This day is no longer in the forecast');
  const section = `${areaTitle(data)} - ${weekday(day.date)}`;
  const items = day.hourly.map(hour => {
    const description = hour.weatherDesc?.[0]?.value?.trim() || '?';
    const thunder = hourlyThunder(hour);
    const wet = hourlyRain(hour, u);
    const windy = `${hour[`windspeed${u.wind}`] || '?'} ${u.windUnit} ${windArrow(hour.winddirDegree)} (${hour.winddir16Point})`;
    return {
      id: `hour:${hour.time}`, section, title: hourLabel(hour.time, u),
      subtitle: `${hour[`temp${u.temp}`] || '?'} ${u.tempUnit}     ${description}`,
      icon: weatherIcon(hour.weatherCode),
      accessories: compact([
        thunder && thunder.value > 0 ? {icon: Icons.Thunder, text: thunder.text, tooltip: `Chance of Thunder ${thunder.text}`} : null,
        wet && wet.value > 0 ? {icon: Icons.Rain, text: wet.text, tooltip: `Rain ${wet.text} (Chance ${wet.chance}%)`} : null,
        {icon: Icons.Wind, text: windy, tooltip: `Wind: ${windy}`, color: 'dim'},
        hour.cloudcover ? {icon: Icons.Cloud, text: `${hour.cloudcover} %`, tooltip: `Cloud Cover: ${hour.cloudcover} %`} : null,
        day.uvIndex ? {icon: Icons.UVIndex, tooltip: `UV Index: ${day.uvIndex}`} : null,
        {icon: Icons.Humidity, text: `${hour.humidity}%`, tooltip: `Humidity: ${hour.humidity}%`},
      ]),
    };
  });
  return list(items, {placeholder: section, context: {day: date, location}});
}

export function loadingView(query) {
  return list([], {placeholder: PLACEHOLDER, empty: {title: query ? `Looking up the weather in ${query}…` : 'Looking up the weather…', icon: 'weather-few-clouds-symbolic'}});
}
export function errorView(message) {
  return list([], {placeholder: PLACEHOLDER, empty: {title: 'Could not fetch data from wttr.in', description: message, icon: 'weather-storm-symbolic'}});
}
