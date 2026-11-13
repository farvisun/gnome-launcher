// Builds the speed test's list view from a result; pure, so it can be tested and previewed.
import {list} from '../../sdk/index.mjs';
import {ACTIVITIES, activityLevels, formatBytes, formatMs, formatSpeed, gauge, median, pingRating, speedParts, speedRating, summary} from './lib.mjs';

const SECTION = 'Speed test · Cloudflare';
const ACTIONS = [
  {id: 'primary', title: 'Copy results'},
  {id: 'again', title: 'Run again'},
  {id: 'website', title: 'Open speed.cloudflare.com'},
];
const COLORS = {download: 'blue', upload: 'purple'};

const pct = fraction => `${Math.round((fraction ?? 0) * 100)}%`;
const mbps = values => values.map(v => Math.round(v / 1e5) / 10);
const time = epoch => new Date(epoch).toLocaleTimeString([], {hour: '2-digit', minute: '2-digit'});

export function blank() {
  return {
    stage: 'meta', done: false, error: null, at: null,
    meta: {}, ping: null, jitter: null, latencies: [],
    download: null, downloadSamples: [], downloadBytes: 0, downloadMs: 0,
    upload: null, uploadSamples: [], uploadBytes: 0, uploadMs: 0,
    progress: {latency: 0, download: 0, upload: 0},
  };
}

/** The status text for a measurement that has not produced a value. */
function pending(r, stage) {
  if (r.stage === stage) return stage === 'meta' ? 'Looking up…' : `Measuring… ${pct(r.progress[stage])}`;
  return r.done ? 'Not measured' : 'Waiting…';
}

function speedItem(r, direction) {
  const bps = r[direction];
  const measuring = r.stage === direction;
  const color = COLORS[direction];
  const samples = r[`${direction}Samples`];
  const rating = bps != null ? speedRating(bps) : null;
  const parts = speedParts(bps);
  return {
    id: direction,
    section: SECTION,
    title: direction === 'download' ? 'Download' : 'Upload',
    subtitle: measuring ? `Measuring… ${pct(r.progress[direction])}` : rating?.label ?? pending(r, direction),
    icon: direction === 'download' ? 'go-down-symbolic' : 'go-up-symbolic',
    tint: color,
    progress: measuring ? r.progress[direction] : undefined,
    accessories: [{text: formatSpeed(bps) ?? (measuring ? 'Measuring…' : '—')}],
    detail: {
      hero: parts
        ? {value: parts.value, unit: parts.unit, progress: gauge(bps), color, caption: measuring ? 'Still measuring; the figure settles as larger files finish' : rating.caption}
        : {value: '—', caption: pending(r, direction)},
      chart: samples.length >= 2 ? {values: mbps(samples), color, caption: 'Speed of each transfer, in Mbps'} : undefined,
      metadata: [
        {label: 'Reported', text: formatSpeed(bps) ?? '—'},
        {label: 'Median', text: formatSpeed(median(samples)) ?? '—'},
        {label: 'Peak', text: formatSpeed(samples.length ? Math.max(...samples) : null) ?? '—'},
        {label: 'Transfers', text: String(samples.length)},
        {label: 'Data used', text: formatBytes(r[`${direction}Bytes`])},
        {label: 'Time', text: `${(r[`${direction}Ms`] / 1000).toFixed(1)} s`},
        {separator: true},
        {label: 'Method', text: 'The 90th percentile of all transfers, after subtracting the ping'},
      ],
    },
    actions: ACTIONS,
  };
}

function pingItem(r) {
  const rating = pingRating(r.ping, r.jitter);
  const measuring = r.stage === 'latency';
  return {
    id: 'ping',
    section: SECTION,
    title: 'Ping',
    subtitle: r.ping != null ? `Jitter ${formatMs(r.jitter)}` : pending(r, 'latency'),
    icon: 'network-transmit-receive-symbolic',
    tint: rating?.color ?? 'accent',
    progress: measuring ? r.progress.latency : undefined,
    accessories: [
      {text: formatMs(r.ping) ?? (measuring ? 'Measuring…' : '—')},
      ...(rating && !measuring ? [{tag: rating.label, color: rating.color}] : []),
    ],
    detail: {
      hero: r.ping != null
        ? {value: String(Math.round(r.ping)), unit: 'ms', color: rating.color, caption: measuring ? 'Still measuring' : rating.caption}
        : {value: '—', caption: pending(r, 'latency')},
      chart: r.latencies.length >= 2 ? {values: r.latencies.map(v => Math.round(v * 10) / 10), color: rating?.color ?? 'accent', caption: 'Each round trip, in ms'} : undefined,
      metadata: [
        {label: 'Median', text: formatMs(r.ping) ?? '—'},
        {label: 'Jitter', text: formatMs(r.jitter) ?? '—'},
        {label: 'Lowest', text: formatMs(r.latencies.length ? Math.min(...r.latencies) : null) ?? '—'},
        {label: 'Highest', text: formatMs(r.latencies.length ? Math.max(...r.latencies) : null) ?? '—'},
        {label: 'Samples', text: String(r.latencies.length)},
      ],
    },
    actions: ACTIONS,
  };
}

function serverItem(r) {
  const m = r.meta;
  const place = [m.coloCity, m.colo].filter(Boolean).join(' · ');
  return {
    id: 'server',
    section: SECTION,
    title: 'Server',
    subtitle: place || pending(r, 'meta'),
    icon: 'network-server-symbolic',
    tint: 'accent',
    accessories: m.protocol ? [{tag: m.protocol, tooltip: 'Connection protocol'}] : [],
    detail: {metadata: [
      {label: 'Location', text: m.coloCity ?? '—'},
      {label: 'Data centre', text: m.colo ?? '—'},
      {label: 'Region', text: m.coloRegion ?? '—'},
      {label: 'Protocol', text: m.protocol ?? '—'},
    ]},
    actions: ACTIONS,
  };
}

function networkItem(r) {
  const m = r.meta;
  return {
    id: 'network',
    section: SECTION,
    title: 'Your Network',
    subtitle: m.isp ?? pending(r, 'meta'),
    icon: 'network-workgroup-symbolic',
    tint: 'accent',
    accessories: m.city ? [{text: [m.city, m.country].filter(Boolean).join(', ')}] : [],
    detail: {metadata: [
      {label: 'Provider', text: m.isp ?? '—'},
      {label: 'ASN', text: m.asn ? `AS${m.asn}` : '—'},
      {label: 'Location', text: [m.city, m.region, m.country].filter(Boolean).join(', ') || '—'},
      {label: 'Public IP', text: m.ip ?? '—'},
    ]},
    actions: ACTIONS,
  };
}

function activityItems(r) {
  const items = ACTIVITIES.map(activity => {
    const levels = activityLevels(activity, r.download, r.upload);
    const best = levels.filter(l => l.ok).at(-1);
    return {
      id: `activity:${activity.id}`,
      section: 'What this connection can do',
      title: activity.title,
      subtitle: best ? `Up to ${best.label}` : 'Not enough bandwidth',
      icon: activity.icon,
      tint: best ? 'green' : 'red',
      accessories: levels.map(l => ({tag: l.label, color: l.ok ? 'green' : 'dim', tooltip: l.ok ? 'Supported' : 'Too slow'})),
      detail: {
        text: `What ${activity.title.toLowerCase()} needs at each quality (↓ download, ↑ upload).`,
        metadata: activity.levels.map(([label, down, up], i) => ({
          label,
          text: `${levels[i].ok ? 'Supported' : 'Too slow'} · needs ${down}${up ? ` ↓ ${up} ↑` : ' ↓'} Mbps`,
          color: levels[i].ok ? 'green' : 'red',
        })),
      },
      actions: ACTIONS,
    };
  });
  const ping = pingRating(r.ping, r.jitter);
  if (ping) {
    items.push({
      id: 'activity:gaming', section: 'What this connection can do', title: 'Online Gaming', subtitle: ping.caption,
      icon: 'input-gaming-symbolic', tint: ping.color,
      accessories: [{tag: ping.label, color: ping.color}],
      detail: {text: 'Games care about ping and how steady it is more than about speed.', metadata: [
        {label: 'Ping', text: formatMs(r.ping)}, {label: 'Jitter', text: formatMs(r.jitter)}, {label: 'Rating', text: ping.label, color: ping.color},
      ]},
      actions: ACTIONS,
    });
  }
  return items;
}

function resultItem(r) {
  const rating = speedRating(r.download);
  return {
    id: 'result',
    section: 'Result',
    title: `${rating.label} connection`,
    subtitle: `Measured at ${time(r.at)}`,
    icon: 'power-profile-performance-symbolic',
    tint: rating.color,
    accessories: [
      {tag: `↓ ${formatSpeed(r.download) ?? '—'}`, color: 'blue'},
      {tag: `↑ ${formatSpeed(r.upload) ?? '—'}`, color: 'purple'},
    ],
    detail: {hero: {value: rating.label, color: rating.color, caption: rating.caption}, text: summary(r)},
    actions: ACTIONS,
  };
}

function errorItem(r) {
  return {
    id: 'error', section: SECTION, title: 'The speed test stopped', subtitle: r.error,
    icon: 'network-offline-symbolic', tint: 'red',
    detail: {text: `${r.error}\n\nCheck your connection and run the test again.`},
    actions: [{id: 'again', title: 'Run again'}, {id: 'website', title: 'Open speed.cloudflare.com'}],
  };
}

export function view(r) {
  const items = [speedItem(r, 'download'), speedItem(r, 'upload'), pingItem(r), serverItem(r), networkItem(r)];
  if (r.error) items.unshift(errorItem(r));
  if (r.done) items.push(...activityItems(r), resultItem(r));
  return list(items, {
    detail: true,
    placeholder: r.done ? 'Speed test results · Enter copies them' : 'Speed test running… Esc stops it',
  });
}

/** Summary text for the clipboard, from the summary helper's flat fields. */
export function flat(r) {
  return {
    download: r.download, upload: r.upload, ping: r.ping, jitter: r.jitter, at: r.at,
    server: [r.meta.coloCity, r.meta.colo].filter(Boolean).join(' · ') || null,
    network: r.meta.isp ? `${r.meta.isp}${r.meta.city ? ` · ${r.meta.city}` : ''}` : null,
  };
}
