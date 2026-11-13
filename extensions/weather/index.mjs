import {serve, update, push, host} from '../../sdk/index.mjs';
import {WeatherError, fetchWeather} from './lib.mjs';
import {dayView, errorView, loadingView, weatherView} from './views.mjs';

const CACHE = 'cache';
const KEEP = 6;
const sleep = (ms, signal) => new Promise((resolve, reject) => {
  const timer = setTimeout(resolve, ms);
  signal.addEventListener('abort', () => { clearTimeout(timer); reject(new Error('Cancelled')); }, {once: true});
});
const key = location => location.trim().toLowerCase();

/** The place to show: what was typed, else the default location, else wttr.in's guess from the IP. */
const locationFor = (query, preferences) => (query ?? '').trim() || String(preferences.defaultquery ?? '').trim();

async function cached(location) {
  const cache = (await host.storage.get(CACHE)) ?? {};
  return cache[key(location)] ?? null;
}
async function remember(location, data) {
  const cache = (await host.storage.get(CACHE)) ?? {};
  cache[key(location)] = {at: Date.now(), data};
  const recent = Object.entries(cache).sort((a, b) => b[1].at - a[1].at).slice(0, KEEP);
  await host.storage.set(CACHE, Object.fromEntries(recent));
}

/** Shows the cached report at once, then the fresh one, as Raycast's cached promise does. */
async function load(location, signal, render) {
  const last = await cached(location);
  let view = last ? render(last.data) : loadingView(location);
  update(view);
  // Typing a city queries after each pause; wait a little longer before asking wttr.in.
  if (!last && location) await sleep(350, signal);
  const heartbeat = setInterval(() => update(view), 4000);
  try {
    const data = await fetchWeather(location, signal);
    await remember(location, data);
    return {data};
  } catch (error) {
    if (signal.aborted) throw error;
    const message = error instanceof WeatherError ? error.message : String(error.message ?? error);
    if (last) {
      await host.toast({style: 'failure', title: 'Could not fetch data from wttr.in', message});
      return {data: last.data};
    }
    return {error: message};
  } finally {
    clearInterval(heartbeat);
  }
}

async function show({query, preferences, context, launch_context, signal}) {
  // A pushed day re-queries with its own context; it keeps showing that day.
  if (context?.day) {
    const last = await cached(context.location ?? '');
    return last ? dayView(last.data, context.day, preferences, context.location) : errorView('The forecast is no longer cached');
  }
  const location = locationFor(query, preferences);
  const day = launch_context?.day;
  const render = data => (day ? dayView(data, day, preferences, location) : weatherView(data, preferences));
  const {data, error} = await load(location, signal, render);
  return error ? errorView(error) : render(data);
}

serve({
  index: {
    run: show,
    query: show,
    action: async ({item, query, preferences}) => {
      if (!item.startsWith('day:')) return null;
      const location = locationFor(query, preferences);
      const last = await cached(location);
      return last ? push(dayView(last.data, item.slice(4), preferences, location)) : null;
    },
  },
});
