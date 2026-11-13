// Pure statistics and formatting for the Speedtest extension.

export function median(values) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const middle = sorted.length >> 1;
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

/** Jitter is the mean absolute difference between consecutive latency samples. */
export function jitter(values) {
  if (values.length < 2) return null;
  let total = 0;
  for (let i = 1; i < values.length; i++) total += Math.abs(values[i] - values[i - 1]);
  return total / (values.length - 1);
}

/** Linear-interpolated percentile, p in [0, 1]. */
export function percentile(values, p) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const index = (sorted.length - 1) * p;
  const low = Math.floor(index);
  const high = Math.ceil(index);
  return sorted[low] + (sorted[high] - sorted[low]) * (index - low);
}

/** The longest duration announced in a Server-Timing header, in ms. */
export function serverTime(header) {
  let longest = 0;
  for (const match of String(header ?? '').matchAll(/dur=([\d.]+)/g)) longest = Math.max(longest, Number(match[1]));
  return longest;
}

/** Bits per second for `bytes` moved in `ms`, or null when too quick to time reliably. */
export function bitsPerSecond(bytes, ms) {
  return ms >= 10 ? bytes * 8 / (ms / 1000) : null;
}

export function formatSpeed(bps) {
  if (bps == null) return null;
  if (bps >= 1e9) return `${(bps / 1e9).toFixed(2)} Gbps`;
  if (bps >= 1e6) return `${(bps / 1e6).toFixed(1)} Mbps`;
  return `${(bps / 1e3).toFixed(0)} kbps`;
}

export function formatMs(ms) {
  return ms == null ? null : `${ms < 10 ? ms.toFixed(1) : Math.round(ms)} ms`;
}

/** Plain-text summary for the clipboard. */
export function summary(result) {
  return [
    `Download: ${formatSpeed(result.download) ?? '—'}`,
    `Upload: ${formatSpeed(result.upload) ?? '—'}`,
    `Ping: ${formatMs(result.ping) ?? '—'}`,
    `Jitter: ${formatMs(result.jitter) ?? '—'}`,
    result.server && `Server: ${result.server}`,
    result.network && `Network: ${result.network}`,
    result.at && `Measured: ${new Date(result.at).toLocaleString()}`,
  ].filter(Boolean).join('\n');
}

// What each activity needs, in Mbps per direction (from Raycast's speedtest thresholds).
export const ACTIVITIES = [
  {id: 'voice', title: 'Voice Call', icon: 'call-start-symbolic', levels: [['SD', 0.05, 0.05], ['HD', 0.1, 0.1]]},
  {id: 'video', title: 'Video Call', icon: 'camera-web-symbolic', levels: [['480p', 1, 1], ['720p', 3, 3], ['1080p', 5, 5], ['4K', 20, 20]]},
  // Streaming only needs the download direction.
  {id: 'stream', title: 'Streaming', icon: 'video-display-symbolic', levels: [['480p', 1.5, 0], ['720p', 5, 0], ['1080p', 10, 0], ['4K', 25, 0]]},
];

/** Which levels of an activity a connection supports: [{label, ok}]. */
export function activityLevels(activity, downloadBps, uploadBps) {
  const down = (downloadBps ?? 0) / 1e6;
  const up = (uploadBps ?? 0) / 1e6;
  return activity.levels.map(([label, needDown, needUp]) => ({label, ok: down >= needDown && up >= needUp}));
}

/** A plain-words grade for a download speed, with a palette colour. */
export function speedRating(bps) {
  const mbps = (bps ?? 0) / 1e6;
  if (mbps >= 100) return {label: 'Very fast', color: 'green', caption: 'Enough for 4K streams on several screens at once'};
  if (mbps >= 25) return {label: 'Fast', color: 'green', caption: 'Enough for 4K streaming and large downloads'};
  if (mbps >= 5) return {label: 'Moderate', color: 'yellow', caption: 'Fine for HD video and calls; large downloads take a while'};
  return {label: 'Slow', color: 'red', caption: 'Video may buffer; calls may drop to lower quality'};
}

/** A grade for latency and its variation, which matter most for calls and games. */
export function pingRating(ms, jitterMs) {
  if (ms == null) return null;
  const worst = ms + 2 * (jitterMs ?? 0);
  if (worst < 30) return {label: 'Excellent', color: 'green', caption: 'Great for gaming and calls'};
  if (worst < 70) return {label: 'Good', color: 'green', caption: 'Fine for calls and most games'};
  if (worst < 150) return {label: 'Fair', color: 'yellow', caption: 'Calls work; fast games may feel laggy'};
  return {label: 'Poor', color: 'red', caption: 'Calls may stutter and games will lag'};
}

/** Speed on a 0–1 log scale where 1 Gbps fills the gauge. */
export function gauge(bps) {
  if (!bps) return 0;
  return Math.min(1, Math.log10(bps / 1e6 + 1) / Math.log10(1001));
}

/** Splits "245.3 Mbps" into value and unit for the large figure in the side pane. */
export function speedParts(bps) {
  const text = formatSpeed(bps);
  if (!text) return null;
  const [value, unit] = text.split(' ');
  return {value, unit};
}

export function formatBytes(bytes) {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${Math.round(bytes / 1e3)} kB`;
}
