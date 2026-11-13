// Builds Open Ports rows from program records; pure, so it can be tested and previewed.
import {list} from '../../sdk/index.mjs';
import {EXPOSURE, widest} from './lib.mjs';

// Open Ports re-reads the socket tables this often while it stays open.
export const REFRESH_MS = 5000;

/** At most three addresses, so tooltips stay short. */
const short = addresses => addresses.length > 3 ? `${addresses.slice(0, 3).join(', ')} +${addresses.length - 3} more` : addresses.join(', ');
export const portLabel = p => `${p.port}${p.proto === 'udp' ? '/udp' : ''}${p.name ? ` ${p.name}` : ''}`;
export const programTitle = p => p.command ?? p.ports.find(x => x.name)?.name ?? `Port ${p.ports[0].port}`;

export function searchText(p) {
  return [programTitle(p), p.cmdline, p.user, p.pid, p.path,
    ...p.ports.flatMap(x => [x.port, x.proto, x.name, EXPOSURE[x.exposure].label, x.exposure, ...x.addresses])]
    .filter(v => v != null).join(' ').toLowerCase();
}

export function programItem(p) {
  const kind = widest(p.ports.flatMap(x => x.addresses));
  // Two tags fit beside the side pane; the pane lists every port.
  const shown = p.ports.slice(0, 2);
  const firstTcp = p.ports.find(x => x.proto === 'tcp');
  const actions = [
    {id: 'primary', title: p.pid && p.mine ? 'Stop program' : 'Stop program (administrator)'},
    {id: 'force', title: 'Force stop'},
    {id: 'copy-port', title: p.ports.length > 1 ? 'Copy ports' : 'Copy port'},
  ];
  if (p.pid) actions.push({id: 'copy-pid', title: 'Copy PID'});
  if (p.cmdline) actions.push({id: 'copy-command', title: 'Copy command line'});
  if (firstTcp) actions.push({id: 'open', title: `Open localhost:${firstTcp.port} in browser`});
  actions.push({id: 'name', title: p.ports[0].name ? 'Rename port' : 'Name this port'}, {id: 'reload', title: 'Reload'});
  return {
    id: p.id,
    section: p.mine ? 'Your Programs' : 'System and Other Users',
    title: programTitle(p),
    subtitle: p.pid ? `PID ${p.pid} · ${p.user}` : `${p.user} · program hidden`,
    icon: p.pid ? 'application-x-executable-symbolic' : 'dialog-password-symbolic',
    tint: EXPOSURE[kind].color,
    accessories: [
      ...shown.map(x => ({tag: portLabel(x), color: EXPOSURE[x.exposure].color, tooltip: `${short(x.addresses)} — ${EXPOSURE[x.exposure].description}`})),
      ...(p.ports.length > shown.length ? [{text: `+${p.ports.length - shown.length}`}] : []),
    ],
    detail: {
      text: p.cmdline || (p.pid ? undefined : 'This program belongs to another user, so its details are hidden. Stopping it asks for an administrator password.'),
      metadata: [
        {label: 'Program', text: p.command ?? 'Hidden'},
        {label: 'User', text: p.mine ? `you (${p.uid})` : `${p.user} (${p.uid})`},
        ...(p.pid ? [{label: 'PID', text: String(p.pid)}] : []),
        ...(p.ppid ? [{label: 'Parent', text: `${p.parent ?? 'unknown'} (${p.ppid})`}] : []),
        ...(p.path ? [{label: 'Path', text: p.path}] : []),
        {separator: true},
        {label: 'Ports', tags: p.ports.slice(0, 20).map(x => ({text: portLabel(x), color: EXPOSURE[x.exposure].color}))},
        {label: 'Exposure', tags: [...new Set(p.ports.map(x => x.exposure))].map(e => ({text: EXPOSURE[e].label, color: EXPOSURE[e].color}))},
        {label: 'Protocols', text: [...new Set(p.ports.map(x => x.proto.toUpperCase()))].join(', ')},
        {label: 'Addresses', text: [...new Set(p.ports.flatMap(x => x.addresses))].slice(0, 12).join(', ')},
      ],
    },
    actions,
  };
}

/** The Open Ports list for programs already filtered by the search. */
export function portsList(found, query = '') {
  const needle = query.trim();
  // Sections must be consecutive: your programs first, then everyone else's.
  const sorted = [...found].sort((a, b) => (b.mine - a.mine) || a.ports[0].port - b.ports[0].port);
  return list(sorted.slice(0, 100).map(programItem), {
    detail: true,
    refresh: REFRESH_MS,
    placeholder: 'Search ports, programs or users; try “network” or “udp”',
    empty: needle
      ? {title: `No open port matches “${needle}”`, description: 'Search by port, program, user, or exposure such as “network”', icon: 'network-wired-symbolic'}
      : {title: 'No programs are listening on a port', description: 'Start a server and it appears here within a few seconds', icon: 'network-wired-symbolic'},
  });
}
