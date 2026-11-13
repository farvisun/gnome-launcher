// Pure helpers for the Timers extension; index.mjs does the I/O.

export const MAX_SECONDS = 99 * 3600;
export const DEFAULT_MINUTES = [2, 5, 10, 15, 30, 45, 60, 90];

const UNITS = {h: 3600, hr: 3600, hrs: 3600, hour: 3600, hours: 3600,
  m: 60, min: 60, mins: 60, minute: 60, minutes: 60,
  s: 1, sec: 1, secs: 1, second: 1, seconds: 1};
const UNIT = '(?:hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)';

/**
 * Reads a duration and an optional name from free text, in either order:
 * "5" (minutes), "90s", "1h30m", "1h 30m", "1:30" (m:ss), "1:00:00", "25m tea".
 * Returns {seconds, name} or null when no duration is present.
 */
export function parseDuration(text) {
  const query = String(text ?? '').trim();
  if (!query) return null;
  let seconds;
  let match = query.match(/(?:^|\s)(\d{1,3}):(\d{2})(?::(\d{2}))?(?=\s|$)/);
  if (match) {
    const [a, b, c] = [match[1], match[2], match[3]].map(Number);
    if (b >= 60 || (match[3] !== undefined && c >= 60)) return null;
    seconds = match[3] === undefined ? a * 60 + b : a * 3600 + b * 60 + c;
  } else if ((match = query.match(new RegExp(`(?:^|\\s)((?:\\d+(?:\\.\\d+)?\\s*${UNIT}\\s*)+)(?=\\s|$)`, 'i')))) {
    seconds = 0;
    for (const part of match[1].matchAll(new RegExp(`(\\d+(?:\\.\\d+)?)\\s*(${UNIT})`, 'gi'))) {
      seconds += Number(part[1]) * UNITS[part[2].toLowerCase()];
    }
  } else if ((match = query.match(/(?:^|\s)(\d+(?:\.\d+)?)(?=\s|$)/))) {
    seconds = Number(match[1]) * 60;
  } else {
    return null;
  }
  seconds = Math.round(seconds);
  if (!(seconds > 0 && seconds <= MAX_SECONDS)) return null;
  const name = (query.slice(0, match.index) + ' ' + query.slice(match.index + match[0].length)).replace(/\s+/g, ' ').trim();
  return {seconds, name};
}

/** "1 h 30 min", "2 min 30 s", "45 s". */
export function formatDuration(seconds) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor(seconds % 3600 / 60);
  const s = Math.round(seconds % 60);
  return [h && `${h} h`, m && `${m} min`, s && `${s} s`].filter(Boolean).join(' ') || '0 s';
}

/** A countdown or stopwatch reading: "4:05", "1:02:03". */
export function formatClock(ms) {
  const total = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor(total % 3600 / 60);
  const s = String(total % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

/** Local wall-clock time such as "14:32". */
export function formatTime(epochMs) {
  const date = new Date(epochMs);
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

export function emptyState() {
  return {timers: [], stopwatches: [], presets: []};
}

/** A paused timer keeps its remaining time instead of an end time. */
export const isPaused = timer => typeof timer.pausedLeftMs === 'number';

/** Milliseconds a timer has left, whether running or paused. */
export function timeLeft(timer, now) {
  return isPaused(timer) ? timer.pausedLeftMs : Math.max(0, timer.endsAt - now);
}

/** How much of a timer has elapsed, 0–1, for its progress bar. */
export function fractionDone(timer, now) {
  const total = timer.seconds * 1000;
  return total > 0 ? Math.min(1, Math.max(0, 1 - timeLeft(timer, now) / total)) : 1;
}

/** Validates stored state and drops timers that have already rung. Running timers sort by end time, paused ones after. */
export function normalize(state, now) {
  const value = state && typeof state === 'object' ? state : {};
  const list = key => Array.isArray(value[key]) ? value[key].filter(x => x && typeof x.id === 'string') : [];
  return {
    timers: list('timers')
      .filter(t => isPaused(t) || t.endsAt > now)
      .sort((a, b) => (isPaused(a) - isPaused(b)) || timeLeft(a, now) - timeLeft(b, now)),
    stopwatches: list('stopwatches'),
    presets: list('presets').sort((a, b) => a.seconds - b.seconds),
  };
}

/** Stopwatches store accumulated time plus the start of the current run (null while paused). */
export function elapsed(stopwatch, now) {
  return (stopwatch.accumulatedMs ?? 0) + (stopwatch.startedAt ? now - stopwatch.startedAt : 0);
}

/** Notification body text: GNOME Shell reads a small markup subset there. */
export function escapeMarkup(text) {
  return String(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
