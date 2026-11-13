// Pure helpers for the top-bar weather, shared with Node tests. They follow
// Raycast's Weather Menu Bar and read the same preferences Lumen's Weather
// extension uses (state.json → preferences["lumen.weather"]).

export const WEATHER_ID = 'lumen.weather';
/** How often the top bar asks wttr.in again, as Raycast's menu bar does. */
export const REFRESH_MS = 30 * 60 * 1000;

function parse(text) {
    try { return JSON.parse(text); } catch { return null; }
}

/** Whether Weather is on, and its preferences with Raycast's defaults. */
export function readPrefs(stateText) {
    const state = parse(stateText) ?? {};
    const saved = state.preferences?.[WEATHER_ID] ?? {};
    const value = (name, fallback) => (saved[name] ?? fallback);
    return {
        enabled: Array.isArray(state.enabled_builtins) && state.enabled_builtins.includes(WEATHER_ID),
        location: String(value('defaultquery', '')).trim(),
        imperial: value('unitsystem', 'metric') === 'imperial',
        clock: value('clockformat', '12h') === '24h' ? '24h' : '12h',
        showIcon: value('showmenuicon', true) !== false,
        showText: value('showmenutext', true) !== false,
        feelsLike: value('menutemperature', 'actual') === 'feelslike',
    };
}

export function weatherUrl(location) {
    return `https://wttr.in/${encodeURIComponent(location ?? '')}?format=j1`;
}

/** wttr.in's JSON, unwrapped from its `data` property, or null if unusable. */
export function readWeather(text) {
    const json = parse(text);
    const data = json?.data ?? json;
    const ok = Array.isArray(data?.current_condition) && data.current_condition.length > 0
        && Array.isArray(data.weather) && data.weather.length > 0;
    return ok ? data : null;
}

const CODES = {
    'weather-clear-symbolic': [113],
    'weather-few-clouds-symbolic': [116],
    'weather-overcast-symbolic': [119, 122],
    'weather-fog-symbolic': [143, 248, 260],
    'weather-showers-scattered-symbolic': [176, 179, 182, 185, 263, 266, 281, 284, 293, 296, 311, 314, 317, 350, 353, 362, 365, 374, 377],
    'weather-showers-symbolic': [299, 302, 305, 308, 356, 359],
    'weather-snow-symbolic': [227, 230, 320, 323, 326, 329, 332, 335, 338, 368, 371, 395],
    'weather-storm-symbolic': [200, 386, 389, 392],
};
export function weatherIcon(code) {
    const n = Number(code);
    for (const [icon, codes] of Object.entries(CODES)) if (codes.includes(n)) return icon;
    return 'weather-fog-symbolic';
}

const ARROWS = ['↓', '↙', '←', '↖', '↑', '↗', '→', '↘'];
const arrow = degree => ARROWS[Math.floor(((parseFloat(degree) + 22.5) % 360) / 45)] ?? '?';
const first = list => (Array.isArray(list) && list.length ? list[0].value : undefined);

export function clockTime(text, clock) {
    const t = String(text ?? '').trim();
    const match = t.match(/^(\d{1,2}):(\d{2})\s*(AM|PM)$/i);
    if (clock === '12h' || !match) return t;
    const hour = (Number(match[1]) % 12) + (match[3].toUpperCase() === 'PM' ? 12 : 0);
    return `${String(hour).padStart(2, '0')}:${match[2]}`;
}
function weekday(date) {
    const d = new Date(`${date}T12:00:00`);
    return Number.isNaN(d.getTime()) ? date : d.toLocaleDateString('en-US', {weekday: 'long'});
}
function relative(then, now) {
    const minutes = Math.round((now - then) / 60000);
    if (minutes < 1) return 'just now';
    if (minutes < 60) return minutes === 1 ? '1 minute ago' : `${minutes} minutes ago`;
    const hours = Math.round(minutes / 60);
    return hours === 1 ? '1 hour ago' : `${hours} hours ago`;
}

/**
 * The panel label and the menu, as sections of `{icon, title, value, tooltip,
 * day?, url?, settings?}` rows. `fetchedAt` is when wttr.in answered.
 */
export function menuModel(data, prefs, fetchedAt, now = Date.now()) {
    const t = prefs.imperial ? 'F' : 'C';
    const unit = prefs.imperial ? '°F' : '°C';
    const conditions = data.current_condition[0];
    const today = data.weather[0];
    const temp = conditions[`temp_${t}`] ? `${conditions[`temp_${t}`]} ${unit}` : '?';
    const feels = conditions[`FeelsLike${t}`] ? `${conditions[`FeelsLike${t}`]} ${unit}` : undefined;
    const description = first(conditions.weatherDesc)?.trim();
    const icon = weatherIcon(conditions.weatherCode);
    const area = data.nearest_area?.[0];
    const place = [first(area?.areaName), first(area?.region), first(area?.country)].filter(n => n?.trim()).join(', ');
    const row = (title, value, rowIcon, extra = {}) => (value ? {title, value, icon: rowIcon, ...extra} : null);
    const speed = conditions[`windspeed${prefs.imperial ? 'Miles' : 'Kmph'}`];
    const precip = prefs.imperial ? conditions.precipInches : conditions.precipMM;
    const astronomy = today.astronomy?.[0] ?? {};
    const min = today[`mintemp${t}`];
    const max = today[`maxtemp${t}`];
    const sections = [
        {title: 'Weather', items: [row('Condition', description, icon)]},
        {title: 'Temperature', items: [
            row('Temperature', temp, 'weather-clear-symbolic'),
            row('Feels Like', feels, 'weather-clear-symbolic'),
            row('Min', min && `${min} ${unit}`, 'go-down-symbolic'),
            row('Max', max && `${max} ${unit}`, 'go-up-symbolic'),
        ]},
        {title: 'Air', items: [
            row('Wind', speed && `${speed} ${prefs.imperial ? 'mph' : 'km/h'} ${arrow(conditions.winddirDegree)} (${conditions.winddir16Point})`, 'weather-windy-symbolic'),
            row('Rain', precip && `${Number(precip)} ${prefs.imperial ? 'Inches' : 'mm'}`, 'weather-showers-scattered-symbolic'),
            row('Pressure', (prefs.imperial ? conditions.pressureInches : conditions.pressure) && `${prefs.imperial ? conditions.pressureInches : conditions.pressure} ${prefs.imperial ? 'PSI' : 'hPa'}`, null),
            row('Humidity', conditions.humidity && `${conditions.humidity} %`, null),
            row('Cloud Cover', conditions.cloudcover && `${conditions.cloudcover} %`, 'weather-overcast-symbolic'),
            row('UV Index', conditions.uvIndex, 'weather-clear-symbolic'),
            row('Visibility', (prefs.imperial ? conditions.visibilityMiles : conditions.visibility) && `${prefs.imperial ? conditions.visibilityMiles : conditions.visibility} ${prefs.imperial ? 'Miles' : 'Km'}`, null),
        ]},
        {title: 'Sun', items: [
            row('Sun Hours', today.sunHour && `${today.sunHour} h`, 'weather-clear-symbolic'),
            row('Sunrise', clockTime(astronomy.sunrise, prefs.clock), 'daytime-sunrise-symbolic'),
            row('Sunset', clockTime(astronomy.sunset, prefs.clock), 'daytime-sunset-symbolic'),
        ]},
        {title: 'Moon', items: [
            row('Moonrise', clockTime(astronomy.moonrise, prefs.clock), 'weather-clear-night-symbolic', {tooltip: `Phase: ${astronomy.moon_phase}`}),
            row('Moonset', clockTime(astronomy.moonset, prefs.clock), 'weather-clear-night-symbolic', {tooltip: `Phase: ${astronomy.moon_phase}`}),
        ]},
        {title: 'Forecast', items: data.weather.map(day => {
            const parts = [`⬆ ${day[`maxtemp${t}`] || '?'} ${unit}`, `⬇ ${day[`mintemp${t}`] || '?'} ${unit}`];
            if (day.sunHour) parts.push(`☀ ${day.sunHour}h`);
            if (Number(day.totalSnow_cm) > 0) parts.push(`❄ ${Number(day.totalSnow_cm)} cm`);
            const counts = new Map();
            for (const hour of day.hourly ?? []) counts.set(hour.weatherCode, (counts.get(hour.weatherCode) ?? 0) + 1);
            const code = [...counts].sort((a, b) => b[1] - a[1])[0]?.[0];
            return {title: weekday(day.date), value: parts.join(' '), icon: weatherIcon(code), day: day.date};
        })},
        {title: 'Location', items: [
            row('Area', first(area?.areaName), 'mark-location-symbolic'),
            row('Region', first(area?.region), null),
            row('Country', first(area?.country), null),
            row('Lon, Lat', area?.longitude && area?.latitude && `${area.longitude},${area.latitude}`, null),
        ]},
        {title: null, items: [
            {title: 'Source', value: 'wttr.in', icon: 'web-browser-symbolic', url: 'https://wttr.in'},
            row('Fetched', fetchedAt ? relative(fetchedAt, now) : '-', null),
            {title: 'Settings…', value: '', icon: 'emblem-system-symbolic', settings: true},
        ]},
    ].map(section => ({...section, items: section.items.filter(Boolean)})).filter(section => section.items.length);
    const shown = prefs.feelsLike ? feels ?? temp : temp;
    // With neither icon nor text chosen, Raycast still shows a cloud so the menu stays reachable.
    const panelIcon = prefs.showIcon || !prefs.showText ? (prefs.showIcon ? icon : 'weather-overcast-symbolic') : null;
    return {
        panel: {icon: panelIcon, text: prefs.showText ? shown : '', tooltip: description ? `${description} · ${place}` : place},
        sections,
    };
}
