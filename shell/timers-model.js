// Pure helpers for the top-bar timer countdown, shared with Node tests.
// The Timers extension writes extension-storage/lumen.timers.json as
// {"state": {"timers": [...], "stopwatches": [...], "presets": [...]}}.

export const TIMERS_ID = 'lumen.timers';

function parse(text) {
    try { return JSON.parse(text); } catch { return null; }
}

/** Whether the built-in Timers extension is turned on in Lumen's state.json. */
export function timersEnabled(stateText) {
    const state = parse(stateText);
    return Array.isArray(state?.enabled_builtins) && state.enabled_builtins.includes(TIMERS_ID);
}

/** Running timers (soonest first), paused timers and stopwatches, with `active` when anything is counting. */
export function readTimers(storageText, now) {
    const state = parse(storageText)?.state ?? {};
    const all = (Array.isArray(state.timers) ? state.timers : []).filter(t => t && typeof t === 'object');
    const timers = all
        .filter(t => typeof t.pausedLeftMs !== 'number' && typeof t.endsAt === 'number' && t.endsAt > now)
        .map(t => ({name: String(t.name ?? 'Timer'), endsAt: t.endsAt}))
        .sort((a, b) => a.endsAt - b.endsAt);
    // Paused timers keep their remaining time; they show in the menu but never tick.
    const paused = all
        .filter(t => typeof t.pausedLeftMs === 'number')
        .map(t => ({name: String(t.name ?? 'Timer'), leftMs: t.pausedLeftMs}));
    const stopwatches = (Array.isArray(state.stopwatches) ? state.stopwatches : [])
        .filter(s => s && typeof s.id === 'string')
        .map(s => ({name: String(s.name ?? 'Stopwatch'), startedAt: typeof s.startedAt === 'number' ? s.startedAt : null, accumulatedMs: Number(s.accumulatedMs) || 0}));
    return {timers, paused, stopwatches, active: timers.length > 0 || stopwatches.some(s => s.startedAt !== null)};
}

export function elapsed(stopwatch, now) {
    return stopwatch.accumulatedMs + (stopwatch.startedAt !== null ? now - stopwatch.startedAt : 0);
}

/** "4:05" or "1:02:03"; matches the Timers extension's own clock. */
export function formatClock(ms) {
    const total = Math.max(0, Math.round(ms / 1000));
    const h = Math.floor(total / 3600);
    const m = Math.floor(total % 3600 / 60);
    const s = String(total % 60).padStart(2, '0');
    return h ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

/** The panel shows the next timer to ring, or else the first running stopwatch. */
export function panelText(model, now) {
    if (model.timers.length) return formatClock(model.timers[0].endsAt - now);
    const running = model.stopwatches.find(s => s.startedAt !== null);
    return running ? formatClock(elapsed(running, now)) : '';
}
