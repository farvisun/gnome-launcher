import {execFile} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {promisify} from 'node:util';
import {serve, list, form, host} from '../../sdk/index.mjs';
import {DEFAULT_MINUTES, elapsed, emptyState, escapeMarkup, formatClock, formatDuration, formatTime, fractionDone, isPaused, normalize, parseDuration, timeLeft} from './lib.mjs';

const exec = promisify(execFile);
// The Shell's top-bar countdown reads this key from lumen.timers.json.
const KEY = 'state';
// A rename form remembers what it renames here, since forms carry only their fields.
const PENDING = 'pending-rename';
// Lists with something counting repaint every second, like Raycast's live lists.
const TICK_MS = 1000;
const newId = () => randomUUID().slice(0, 12);

async function load() {
  return normalize(await host.storage.get(KEY) ?? emptyState(), Date.now());
}
const save = state => host.storage.set(KEY, state);

/**
 * A transient systemd user timer rings the alarm, so it fires even when Lumen
 * is closed. Arguments go to systemd as a list; no shell parses the name.
 */
async function schedule(name, delaySeconds, totalSeconds, signal) {
  const unit = `lumen-timer-${newId()}`;
  try {
    await exec('systemd-run', [
      '--user', '--collect', '--quiet', `--unit=${unit}`, `--description=Lumen timer: ${name}`,
      `--on-active=${Math.max(1, Math.round(delaySeconds))}s`,
      // systemd may otherwise coalesce wake-ups by up to a minute.
      '--timer-property=AccuracySec=100ms',
      '--', 'notify-send', '--app-name=Lumen', '--urgency=critical', '--icon=alarm-symbolic',
      '--hint=string:sound-name:complete', '--hint=string:desktop-entry:io.github.gnomelauncher.Launcher',
      '--', 'Timer finished', escapeMarkup(`${name} · ${formatDuration(totalSeconds)}`),
    ], {signal, timeout: 8000});
  } catch (error) {
    throw new Error(`Could not schedule the timer: ${error.stderr?.trim() || error.message}`);
  }
  return unit;
}
async function unschedule(timer) {
  // The unit is already gone once the timer has rung or while it is paused.
  if (timer.unit) await exec('systemctl', ['--user', 'stop', `${timer.unit}.timer`], {timeout: 8000}).catch(() => {});
}
async function newTimer(name, seconds, signal) {
  const label = name || `${formatDuration(seconds)} timer`;
  return {id: newId(), name: label, seconds, endsAt: Date.now() + seconds * 1000, unit: await schedule(label, seconds, seconds, signal), startedAt: Date.now()};
}

// ---- Timers -------------------------------------------------------------

const TIMER_ACTIONS = timer => [
  {id: 'primary', title: isPaused(timer) ? 'Resume timer' : 'Pause timer'},
  {id: 'stop', title: 'Stop timer'},
  {id: 'restart', title: 'Restart timer'},
  {id: 'rename', title: 'Rename timer'},
  {id: 'preset', title: 'Save as preset'},
  {id: 'copy', title: 'Copy time left'},
];

function timerItem(timer, now) {
  const paused = isPaused(timer);
  const color = paused ? 'red' : 'yellow';
  return {
    id: `timer:${timer.id}`,
    section: 'Timers',
    title: timer.name,
    subtitle: `${formatClock(timeLeft(timer, now))} left`,
    icon: 'alarm-symbolic',
    tint: color,
    progress: fractionDone(timer, now),
    accessories: [
      {text: formatDuration(timer.seconds), icon: 'preferences-system-time-symbolic', tooltip: 'Original length'},
      paused ? {text: 'Paused'} : {text: `Ends ${formatTime(timer.endsAt)}`, tooltip: 'Rings at this time'},
      {tag: paused ? 'Paused' : 'Running', color},
    ],
    actions: TIMER_ACTIONS(timer),
  };
}

function presetItem(preset) {
  return {
    id: `preset:${preset.id}`,
    section: 'Saved Presets',
    title: preset.name,
    icon: 'starred-symbolic',
    tint: 'accent',
    accessories: [{text: formatDuration(preset.seconds)}],
    actions: [{id: 'primary', title: 'Start timer'}, {id: 'rename', title: 'Rename preset'}, {id: 'delete', title: 'Delete preset'}],
  };
}

/** Start Timer and Manage Timers share one view; Start Timer adds quick presets. */
async function timersView({query = ''}, withQuickStart) {
  const state = await load();
  const now = Date.now();
  const parsed = parseDuration(query);
  const needle = parsed ? '' : query.trim().toLowerCase();
  const matches = text => !needle || text.toLowerCase().includes(needle);
  const items = [];
  if (parsed) {
    const name = parsed.name || `${formatDuration(parsed.seconds)} timer`;
    items.push({
      id: 'custom', section: 'New Timer', title: `Start “${name}”`,
      subtitle: parsed.name ? formatDuration(parsed.seconds) : 'Add a name after the duration, such as “25m tea”',
      icon: 'alarm-symbolic', tint: 'green',
      accessories: [{text: `Ends ${formatTime(now + parsed.seconds * 1000)}`}, {tag: formatDuration(parsed.seconds), color: 'green'}],
      actions: [{id: 'primary', title: 'Start timer'}, {id: 'save', title: 'Save as preset'}],
    });
  }
  items.push(...state.timers.filter(t => matches(t.name)).map(t => timerItem(t, now)));
  items.push(...state.presets.filter(p => matches(p.name)).map(presetItem));
  if (withQuickStart) {
    items.push(...DEFAULT_MINUTES.map(minutes => ({
      id: `default:${minutes}`, section: 'Quick Start', title: `${formatDuration(minutes * 60)} timer`,
      icon: 'alarm-symbolic', tint: 'dim',
      accessories: [{text: `Ends ${formatTime(now + minutes * 60_000)}`}],
      actions: [{id: 'primary', title: 'Start timer'}],
    })).filter(item => matches(item.title)));
  }
  return list(items, {
    placeholder: withQuickStart ? 'Type a duration, such as 25m tea or 1:30' : 'Search timers, or type a duration to start one',
    refresh: state.timers.some(t => !isPaused(t)) ? TICK_MS : undefined,
    empty: needle
      ? {title: `No timer matches “${query.trim()}”`, description: 'Type a duration such as 5m, 1h30m or 1:30 to start one', icon: 'alarm-symbolic'}
      : {title: 'No timers running', description: 'Type a duration such as 25m tea and press Enter to start one', icon: 'alarm-symbolic'},
  });
}

async function timersAction(context, withQuickStart) {
  const {item, action, query = '', signal} = context;
  const state = await load();
  const now = Date.now();
  const back = () => timersView({query: ''}, withQuickStart);
  if (item === 'custom') {
    const parsed = parseDuration(query);
    if (!parsed) return null;
    if (action === 'save') {
      state.presets.push({id: newId(), name: parsed.name || `${formatDuration(parsed.seconds)} timer`, seconds: parsed.seconds});
    } else {
      state.timers.push(await newTimer(parsed.name, parsed.seconds, signal));
    }
    await save(state);
    return back();
  }
  if (item.startsWith('default:')) {
    state.timers.push(await newTimer('', Number(item.slice(8)) * 60, signal));
    await save(state);
    return back();
  }
  if (item.startsWith('preset:')) {
    const preset = state.presets.find(p => `preset:${p.id}` === item);
    if (!preset) return back();
    if (action === 'rename') return renameForm('preset', preset.id, preset.name);
    if (action === 'delete') state.presets = state.presets.filter(p => p !== preset);
    else state.timers.push(await newTimer(preset.name, preset.seconds, signal));
    await save(state);
    return back();
  }
  const timer = state.timers.find(t => `timer:${t.id}` === item);
  if (!timer) return back();
  switch (action) {
    case 'copy': await host.copy(formatClock(timeLeft(timer, now))); return null;
    case 'rename': return renameForm('timer', timer.id, timer.name);
    case 'preset':
      state.presets.push({id: newId(), name: timer.name, seconds: timer.seconds});
      break;
    case 'stop':
      await unschedule(timer);
      state.timers = state.timers.filter(t => t !== timer);
      break;
    case 'restart':
      await unschedule(timer);
      Object.assign(timer, {endsAt: now + timer.seconds * 1000, pausedLeftMs: undefined, unit: await schedule(timer.name, timer.seconds, timer.seconds, signal)});
      break;
    default:
      if (isPaused(timer)) {
        const left = timer.pausedLeftMs;
        Object.assign(timer, {endsAt: now + left, pausedLeftMs: undefined, unit: await schedule(timer.name, left / 1000, timer.seconds, signal)});
      } else {
        await unschedule(timer);
        Object.assign(timer, {pausedLeftMs: timeLeft(timer, now), unit: null});
      }
  }
  await save(state);
  return back();
}

// ---- Stopwatches --------------------------------------------------------

function stopwatchItem(watch, now) {
  const running = Boolean(watch.startedAt);
  const color = running ? 'green' : 'red';
  return {
    id: `watch:${watch.id}`,
    section: 'Stopwatches',
    title: watch.name,
    subtitle: `${formatClock(elapsed(watch, now))} elapsed`,
    icon: 'preferences-system-time-symbolic',
    tint: color,
    accessories: [
      {text: `Started ${formatTime(watch.createdAt ?? watch.startedAt ?? now)}`},
      {tag: running ? 'Running' : 'Paused', color},
    ],
    actions: [
      {id: 'primary', title: running ? 'Pause stopwatch' : 'Resume stopwatch'},
      {id: 'reset', title: 'Restart stopwatch'},
      {id: 'rename', title: 'Rename stopwatch'},
      {id: 'copy', title: 'Copy elapsed time'},
      {id: 'delete', title: 'Delete stopwatch'},
    ],
  };
}

async function stopwatchesView({query = ''}, withStart) {
  const state = await load();
  const now = Date.now();
  const name = query.trim();
  const items = [];
  if (withStart || name) {
    items.push({
      id: 'start', section: 'New Stopwatch', title: `Start “${name || `Stopwatch ${state.stopwatches.length + 1}`}”`,
      subtitle: name ? 'Press Enter to start counting' : 'Type a name first, or start an untitled one',
      icon: 'media-playback-start-symbolic', tint: 'green',
      actions: [{id: 'primary', title: 'Start stopwatch'}],
    });
  }
  const needle = withStart ? '' : name.toLowerCase();
  items.push(...state.stopwatches.filter(s => !needle || s.name.toLowerCase().includes(needle)).map(s => stopwatchItem(s, now)));
  return list(items, {
    placeholder: withStart ? 'Name the new stopwatch' : 'Search stopwatches, or type a name to start one',
    refresh: state.stopwatches.some(s => s.startedAt) ? TICK_MS : undefined,
    empty: {title: 'No stopwatches yet', description: 'Type a name and press Enter to start one', icon: 'preferences-system-time-symbolic'},
  });
}

async function stopwatchesAction({item, action, query = ''}, withStart) {
  const state = await load();
  const now = Date.now();
  if (item === 'start') {
    state.stopwatches.push({id: newId(), name: query.trim() || `Stopwatch ${state.stopwatches.length + 1}`, startedAt: now, createdAt: now, accumulatedMs: 0});
    await save(state);
    return stopwatchesView({query: ''}, false);
  }
  const watch = state.stopwatches.find(s => `watch:${s.id}` === item);
  if (!watch) return stopwatchesView({query}, withStart);
  if (action === 'copy') {
    await host.copy(formatClock(elapsed(watch, now)));
    return null;
  }
  if (action === 'rename') return renameForm('stopwatch', watch.id, watch.name);
  if (action === 'delete') state.stopwatches = state.stopwatches.filter(s => s !== watch);
  else if (action === 'reset') Object.assign(watch, {accumulatedMs: 0, startedAt: now, createdAt: now});
  else if (watch.startedAt) Object.assign(watch, {accumulatedMs: elapsed(watch, now), startedAt: null});
  else watch.startedAt = now;
  await save(state);
  return stopwatchesView({query: ''}, false);
}

// ---- Renaming -----------------------------------------------------------

async function renameForm(kind, id, name) {
  await host.storage.set(PENDING, {kind, id});
  return form(`Rename ${kind}`, [{id: 'name', label: 'Name', type: 'text', required: true, value: name}], 'Rename');
}
async function rename({values}) {
  const pending = await host.storage.get(PENDING);
  const name = String(values?.name ?? '').trim().slice(0, 80);
  const state = await load();
  const collection = {timer: state.timers, preset: state.presets, stopwatch: state.stopwatches}[pending?.kind] ?? [];
  const target = collection.find(x => x.id === pending?.id);
  if (target && name) {
    target.name = name;
    await save(state);
  }
  await host.storage.set(PENDING, null);
  return pending?.kind === 'stopwatch' ? stopwatchesView({query: ''}, false) : timersView({query: ''}, pending?.kind === 'preset');
}

serve({
  'start-timer': {run: c => timersView(c, true), action: c => timersAction(c, true), submit: rename},
  'manage-timers': {run: c => timersView(c, false), action: c => timersAction(c, false), submit: rename},
  'start-stopwatch': {run: c => stopwatchesView(c, true), action: c => stopwatchesAction(c, true), submit: rename},
  'manage-stopwatches': {run: c => stopwatchesView(c, false), action: c => stopwatchesAction(c, false), submit: rename},
});
