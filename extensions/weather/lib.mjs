// wttr.in data and formatting, ported from Raycast's Weather extension.
// Everything here is pure except `fetchWeather`.
import {fileURLToPath} from 'node:url';

const ICONS = fileURLToPath(new URL('./icons/', import.meta.url));
/** Icons Adwaita lacks, shipped beside the extension as symbolic SVGs. */
export const icon = name => `${ICONS}${name}-symbolic.svg`;

export const Icons = {
  FeelsLike: icon('thermometer'),
  Temperature: icon('thermometer'),
  Humidity: icon('humidity'),
  Pressure: icon('pressure'),
  Visibility: icon('visibility'),
  Rain: icon('droplet'),
  UVIndex: icon('uv'),
  Moonrise: icon('moonrise'),
  Moonset: icon('moonset'),
  Moon: 'weather-clear-night-symbolic',
  Sun: 'weather-clear-symbolic',
  SunHours: 'weather-clear-symbolic',
  Sunrise: 'daytime-sunrise-symbolic',
  Sunset: 'daytime-sunset-symbolic',
  Cloud: 'weather-overcast-symbolic',
  Snow: 'weather-snow-symbolic',
  Thunder: 'weather-storm-symbolic',
  Wind: 'weather-windy-symbolic',
  Coordinates: 'mark-location-symbolic',
  ArrowUp: 'go-up-symbolic',
  ArrowDown: 'go-down-symbolic',
};

// Codes from https://github.com/chubin/wttr.in/blob/master/lib/constants.py
export const WWO_CODE = {
  113: 'Sunny', 116: 'PartlyCloudy', 119: 'Cloudy', 122: 'VeryCloudy', 143: 'Fog',
  176: 'LightShowers', 179: 'LightSleetShowers', 182: 'LightSleet', 185: 'LightSleet',
  200: 'ThunderyShowers', 227: 'LightSnow', 230: 'HeavySnow', 248: 'Fog', 260: 'Fog',
  263: 'LightShowers', 266: 'LightRain', 281: 'LightSleet', 284: 'LightSleet',
  293: 'LightRain', 296: 'LightRain', 299: 'HeavyShowers', 302: 'HeavyRain',
  305: 'HeavyShowers', 308: 'HeavyRain', 311: 'LightSleet', 314: 'LightSleet',
  317: 'LightSleet', 320: 'LightSnow', 323: 'LightSnowShowers', 326: 'LightSnowShowers',
  329: 'HeavySnow', 332: 'HeavySnow', 335: 'HeavySnowShowers', 338: 'HeavySnow',
  350: 'LightSleet', 353: 'LightShowers', 356: 'HeavyShowers', 359: 'HeavyRain',
  362: 'LightSleetShowers', 365: 'LightSleetShowers', 368: 'LightSnowShowers',
  371: 'HeavySnowShowers', 374: 'LightSleetShowers', 377: 'LightSleet',
  386: 'ThunderyShowers', 389: 'ThunderyHeavyRain', 392: 'ThunderySnowShowers', 395: 'HeavySnowShowers',
};
const SYMBOL = {
  Sunny: 'weather-clear-symbolic',
  PartlyCloudy: 'weather-few-clouds-symbolic',
  Cloudy: 'weather-overcast-symbolic',
  VeryCloudy: 'weather-overcast-symbolic',
  Fog: 'weather-fog-symbolic',
  LightRain: 'weather-showers-scattered-symbolic',
  LightShowers: 'weather-showers-scattered-symbolic',
  LightSleet: 'weather-showers-scattered-symbolic',
  LightSleetShowers: 'weather-showers-scattered-symbolic',
  HeavyRain: 'weather-showers-symbolic',
  HeavyShowers: 'weather-showers-symbolic',
  LightSnow: 'weather-snow-symbolic',
  LightSnowShowers: 'weather-snow-symbolic',
  HeavySnow: 'weather-snow-symbolic',
  HeavySnowShowers: 'weather-snow-symbolic',
  ThunderyShowers: 'weather-storm-symbolic',
  ThunderyHeavyRain: 'weather-storm-symbolic',
  ThunderySnowShowers: 'weather-storm-symbolic',
};
export function weatherIcon(code) {
  // Haze and smog have no WWO code; fog is the closest picture.
  return SYMBOL[WWO_CODE[code]] ?? 'weather-fog-symbolic';
}
export const WIND_DIRECTION = ['↓', '↙', '←', '↖', '↑', '↗', '→', '↘'];
export function windArrow(degree) {
  const deg = parseFloat(degree);
  return Number.isNaN(deg) ? '?' : WIND_DIRECTION[Math.floor(((deg + 22.5) % 360) / 45)];
}

/** Units for the `unitsystem` preference. */
export function units(prefs = {}) {
  const imperial = prefs.unitsystem === 'imperial';
  return {
    imperial,
    temp: imperial ? 'F' : 'C',
    tempUnit: imperial ? '°F' : '°C',
    wind: imperial ? 'Miles' : 'Kmph',
    windUnit: imperial ? 'mph' : 'km/h',
    clock: prefs.clockformat === '12h' ? '12h' : '24h',
  };
}

export class WeatherError extends Error {}

/** GETs `https://wttr.in/<query>?format=j1`; an empty query lets wttr.in use the IP's location. */
export async function fetchWeather(query, signal) {
  const url = `https://wttr.in/${encodeURIComponent(query ?? '')}?format=j1`;
  let response;
  try {
    response = await fetch(url, {signal, headers: {Accept: 'application/json'}});
  } catch (error) {
    if (signal?.aborted) throw error;
    throw new WeatherError(error.cause?.message ?? error.message);
  }
  let json;
  try {
    json = await response.json();
  } catch {
    throw new WeatherError(`Server-side issue at wttr.in (${response.status} - invalid json). Please try again later`);
  }
  if (!response.ok) throw new WeatherError(json?.error ?? `http status ${response.status}`);
  // wttr.in now wraps its response in a "data" property.
  const data = json?.data ?? json;
  if (!isValid(data)) throw new WeatherError('Invalid weather data received from wttr.in. Please try again later');
  return data;
}
export function isValid(data) {
  return Array.isArray(data?.current_condition) && data.current_condition.length > 0
    && Array.isArray(data.weather) && data.weather.length > 0;
}

const first = list => (Array.isArray(list) && list.length ? list[0].value : undefined);

/** "Berlin, Berlin, Germany", or the coordinates wttr.in was asked for. */
export function areaTitle(data) {
  const area = data?.nearest_area?.[0];
  const names = [first(area?.areaName), first(area?.region), first(area?.country)]
    .filter(n => n != null && n.trim().length > 0).map(n => n.trim());
  // Region often repeats the city ("Berlin, Berlin, Germany"); keep it as Raycast does.
  return names.join(', ') || formatRequestQuery(data?.request?.[0]?.query) || '?';
}
export function formatRequestQuery(query) {
  if (!query) return undefined;
  const match = query.match(/Lat\s+([-\d.]+)\s+and\s+Lon\s+([-\d.]+)/i);
  if (!match) return query;
  const [lat, lon] = [parseFloat(match[1]), parseFloat(match[2])];
  if (Number.isNaN(lat) || Number.isNaN(lon)) return query;
  return `${Math.abs(lat).toFixed(2)}°${lat >= 0 ? 'N' : 'S'}, ${Math.abs(lon).toFixed(2)}°${lon >= 0 ? 'E' : 'W'}`;
}
export function areaValues(data) {
  const area = data?.nearest_area?.[0];
  if (!area) return undefined;
  return {areaName: first(area.areaName), region: first(area.region), country: first(area.country), latitude: area.latitude, longitude: area.longitude};
}

export const current = data => data?.current_condition?.[0];
export const today = data => data?.weather?.[0];

export function temperature(conditions, u) {
  const value = conditions?.[`temp_${u.temp}`];
  return `${value || '?'} ${u.tempUnit}`;
}
export function feelsLike(conditions, u) {
  const value = conditions?.[`FeelsLike${u.temp}`];
  return value ? `${value} ${u.tempUnit}` : undefined;
}
export function dayTemperature(day, prefix, u) {
  return `${day?.[`${prefix}temp${u.temp}`] || '?'} ${u.tempUnit}`;
}
export function wind(conditions, u) {
  const speed = conditions?.[`windspeed${u.wind}`];
  if (!speed || !conditions.winddirDegree || !conditions.winddir16Point) return undefined;
  return {speed, unit: u.windUnit, arrow: windArrow(conditions.winddirDegree), dir: conditions.winddir16Point};
}
export function windText(conditions, u) {
  const w = wind(conditions, u);
  return w ? `${w.speed} ${w.unit} ${w.arrow} (${w.dir})` : '?';
}
export function visibility(conditions, u) {
  if (!conditions) return undefined;
  const distance = u.imperial ? conditions.visibilityMiles : conditions.visibility;
  return distance ? `${distance} ${u.imperial ? 'Miles' : 'Km'}` : undefined;
}
export function pressure(conditions, u) {
  const value = u.imperial ? conditions?.pressureInches : conditions?.pressure;
  return value ? `${value} ${u.imperial ? 'PSI' : 'hPa'}` : undefined;
}
export function rain(conditions, u) {
  const text = u.imperial ? conditions?.precipInches : conditions?.precipMM;
  const value = Number(text);
  if (!text || Number.isNaN(value)) return undefined;
  return {value, text: `${value} ${u.imperial ? 'Inches' : 'mm'}`};
}
export function cloudCover(conditions) {
  const value = Number(conditions?.cloudcover);
  return conditions?.cloudcover && !Number.isNaN(value) ? `${value} %` : undefined;
}
export function sunHours(data) {
  const value = today(data)?.sunHour;
  return value ? `${value} h` : undefined;
}
export function minMax(data, u) {
  const day = today(data);
  const min = day?.[`mintemp${u.temp}`];
  const max = day?.[`maxtemp${u.temp}`];
  return min && max ? {min: `${min} ${u.tempUnit}`, max: `${max} ${u.tempUnit}`} : undefined;
}
export function sun(data) {
  const a = today(data)?.astronomy?.[0];
  return a?.sunrise && a?.sunset ? {sunrise: a.sunrise, sunset: a.sunset} : undefined;
}
export function moon(data) {
  const a = today(data)?.astronomy?.[0];
  return a?.moonrise && a?.moonset && a?.moon_phase ? {moonrise: a.moonrise, moonset: a.moonset, phase: a.moon_phase} : undefined;
}
export function snow(day) {
  const value = Number(day?.totalSnow_cm);
  return day?.totalSnow_cm && !Number.isNaN(value) ? {value, text: `${value} cm`} : undefined;
}
export function hourlyRain(hour, u) {
  const text = u.imperial ? hour?.precipInches : hour?.precipMM;
  const value = Number(text);
  if (!text || Number.isNaN(value) || !hour.chanceofrain) return undefined;
  return {value, text: `${text} ${u.imperial ? 'Inches' : 'mm'}`, chance: hour.chanceofrain};
}
export function hourlyThunder(hour) {
  const value = Number(hour?.chanceofthunder);
  return hour?.chanceofthunder && !Number.isNaN(value) ? {value, text: `${hour.chanceofthunder} %`} : undefined;
}

/** The code seen most often in a day's hours, standing for the whole day. */
export function dayCode(day) {
  const counts = new Map();
  for (const hour of day?.hourly ?? []) counts.set(hour.weatherCode, (counts.get(hour.weatherCode) ?? 0) + 1);
  let best; let most = 0;
  for (const [code, count] of counts) if (count > most) { best = code; most = count; }
  return best;
}
export function weekday(date) {
  const d = new Date(`${date}T12:00:00`);
  return Number.isNaN(d.getTime()) ? date : d.toLocaleDateString('en-US', {weekday: 'long'});
}

/** "07:25 AM" in the chosen clock format. */
export function clockTime(text, u) {
  const t = String(text ?? '').trim();
  if (u.clock === '12h') return t;
  const match = t.match(/^(\d{1,2}):(\d{2})\s*(AM|PM)$/i);
  if (!match) return t;
  let hour = Number(match[1]) % 12;
  if (match[3].toUpperCase() === 'PM') hour += 12;
  return `${String(hour).padStart(2, '0')}:${match[2]}`;
}
/** wttr.in's hourly "time" ("0", "300", … "2100") as a clock label. */
export function hourLabel(time, u) {
  const t = parseInt(time, 10) || 0;
  const hour = Math.floor(t / 100);
  const minute = t % 100;
  if (u.clock === '24h') return `${String(hour).padStart(2, '0')}:${String(minute).padStart(2, '0')}`;
  const h12 = hour % 12 === 0 ? 12 : hour % 12;
  return `${h12}${minute ? `:${String(minute).padStart(2, '0')}` : ''} ${hour < 12 ? 'AM' : 'PM'}`;
}

/**
 * When the conditions were observed. wttr.in gives `localObsDateTime` (local
 * to the place) when it has it, and otherwise `observation_time` in UTC.
 */
export function observedAt(conditions, now = Date.now()) {
  const local = conditions?.localObsDateTime?.match(/^(\d{4})-(\d{2})-(\d{2}) (\d{1,2}):(\d{2}) (AM|PM)$/);
  if (local) {
    const [, y, m, d, h, min, ampm] = local;
    return new Date(Number(y), Number(m) - 1, Number(d), (Number(h) % 12) + (ampm === 'PM' ? 12 : 0), Number(min)).getTime();
  }
  const utc = conditions?.observation_time?.match(/^(\d{1,2}):(\d{2}) (AM|PM)$/);
  if (!utc) return undefined;
  const at = new Date(now);
  at.setUTCHours((Number(utc[1]) % 12) + (utc[3] === 'PM' ? 12 : 0), Number(utc[2]), 0, 0);
  // An observation "in the future" was made yesterday (UTC).
  if (at.getTime() > now + 5 * 60_000) at.setUTCDate(at.getUTCDate() - 1);
  return at.getTime();
}
export function relativeTime(then, now = Date.now()) {
  if (then == null || Number.isNaN(then)) return undefined;
  const seconds = Math.round((now - then) / 1000);
  if (seconds < 45) return 'just now';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return minutes === 1 ? '1 minute ago' : `${minutes} minutes ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return hours === 1 ? '1 hour ago' : `${hours} hours ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? 'yesterday' : `${days} days ago`;
}
