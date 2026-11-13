import {serve, update, host} from '../../sdk/index.mjs';
import {bitsPerSecond, jitter, median, percentile, serverTime, summary} from './lib.mjs';
import {blank, flat, view} from './views.mjs';

const BASE = 'https://speed.cloudflare.com';
// speed.cloudflare.com refuses requests that do not come from its own page.
const HEADERS = {Referer: `${BASE}/`};
const LATENCY_SAMPLES = 20;
const PLANS = {
  download: [[100_000, 8], [1_000_000, 6], [10_000_000, 4], [25_000_000, 2]],
  upload: [[100_000, 8], [1_000_000, 6], [10_000_000, 3]],
};
// Each direction stops growing once it has spent this long, so slow links still finish.
const STAGE_BUDGET_MS = 12_000;
// Repaint at most this often while samples stream in.
const PAINT_MS = 250;

async function fetchTimed(url, options, signal) {
  const started = performance.now();
  const response = await fetch(url, {...options, headers: {...HEADERS, ...options?.headers}, cache: 'no-store', signal: AbortSignal.any([signal, AbortSignal.timeout(30_000)])});
  if (!response.ok) throw new Error(`speed.cloudflare.com answered ${response.status}`);
  await response.arrayBuffer();
  return {ms: performance.now() - started, server: serverTime(response.headers.get('server-timing'))};
}

async function readMeta(r, signal) {
  try {
    const meta = await (await fetch(`${BASE}/meta`, {headers: HEADERS, signal})).json();
    const colo = meta.colo && typeof meta.colo === 'object' ? meta.colo : {};
    r.meta = {
      isp: meta.asOrganization ?? null, asn: meta.asn ?? null, ip: meta.clientIp ?? null,
      city: meta.city ?? null, region: meta.region ?? null, country: meta.country ?? null, protocol: meta.httpProtocol ?? null,
      colo: colo.iata ?? (typeof meta.colo === 'string' ? meta.colo : null), coloCity: colo.city ?? null, coloRegion: colo.region ?? null,
    };
  } catch (error) {
    if (signal.aborted) throw error;
  }
}

async function measure(signal) {
  const r = blank();
  let painted = 0;
  const paint = force => {
    if (!force && performance.now() - painted < PAINT_MS) return;
    painted = performance.now();
    update(view(r));
  };
  // Repaint every second even between samples: it keeps the request alive during big transfers.
  const heartbeat = setInterval(() => paint(true), 1000);
  try {
    paint(true);
    await readMeta(r, signal);

    r.stage = 'latency';
    paint(true);
    await fetchTimed(`${BASE}/__down?bytes=0`, {}, signal); // Opens the connection; not timed.
    for (let i = 0; i < LATENCY_SAMPLES; i++) {
      const {ms, server} = await fetchTimed(`${BASE}/__down?bytes=0`, {}, signal);
      r.latencies.push(Math.max(0.1, ms - server));
      r.ping = median(r.latencies);
      r.jitter = jitter(r.latencies);
      r.progress.latency = (i + 1) / LATENCY_SAMPLES;
      paint();
    }
    const rtt = r.ping ?? 0;

    for (const direction of ['download', 'upload']) {
      r.stage = direction;
      paint(true);
      const plan = PLANS[direction];
      const total = plan.reduce((sum, [bytes, count]) => sum + bytes * count, 0);
      const begun = performance.now();
      let moved = 0;
      stages: for (const [bytes, count] of plan) {
        for (let i = 0; i < count; i++) {
          const {ms, server} = direction === 'download'
            ? await fetchTimed(`${BASE}/__down?bytes=${bytes}`, {}, signal)
            : await fetchTimed(`${BASE}/__up`, {method: 'POST', body: new Uint8Array(bytes), headers: {'Content-Type': 'application/octet-stream'}}, signal);
          moved += bytes;
          const speed = bitsPerSecond(bytes, ms - rtt - (direction === 'download' ? server : 0));
          if (speed) r[`${direction}Samples`].push(speed);
          r[direction] = percentile(r[`${direction}Samples`], 0.9);
          r[`${direction}Bytes`] = moved;
          r[`${direction}Ms`] = performance.now() - begun;
          r.progress[direction] = Math.min(0.99, Math.max(moved / total, (performance.now() - begun) / STAGE_BUDGET_MS));
          paint();
          // A transfer this slow means the next size up would take too long.
          if (ms > 2500 || performance.now() - begun > STAGE_BUDGET_MS) break stages;
        }
      }
      r.progress[direction] = 1;
    }
    Object.assign(r, {stage: null, done: true, at: Date.now()});
    await host.storage.set('last', r);
    return view(r);
  } catch (error) {
    if (signal.aborted) throw error;
    Object.assign(r, {stage: null, done: true, at: Date.now(), error: error.message});
    return view(r);
  } finally {
    clearInterval(heartbeat);
  }
}

async function last() {
  const result = await host.storage.get('last');
  // Results saved by the first version had a different shape; run again for those.
  return result && typeof result === 'object' && result.meta && Array.isArray(result.latencies) ? result : null;
}

serve({run: {
  run: ({signal}) => measure(signal),
  // Typing must not restart a test; show the last result instead.
  query: async () => {
    const result = await last();
    return result ? view(result) : null;
  },
  action: async ({action, signal}) => {
    if (action === 'again') return measure(signal);
    if (action === 'website') {
      await host.open(`${BASE}/`);
      return null;
    }
    const result = await last();
    if (!result) throw new Error('No finished speed test to copy yet');
    await host.copy(summary(flat(result)));
    return null;
  },
}});
