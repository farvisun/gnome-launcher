// Pure parsing for the Port Manager extension; index.mjs reads /proc.

const TCP_LISTEN = '0A';
const UDP_UNCONNECTED = '07';

/** Each kernel word is little-endian, so the bytes of every 4-byte group are reversed. */
function bytes(hex) {
  const out = [];
  for (let word = 0; word < hex.length; word += 8) {
    for (let i = 6; i >= 0; i -= 2) out.push(parseInt(hex.slice(word + i, word + i + 2), 16));
  }
  return out;
}

/** RFC 5952 text for 16 address bytes, with IPv4-mapped addresses in dotted form. */
export function formatIPv6(b) {
  if (b.slice(0, 10).every(x => x === 0) && b[10] === 0xff && b[11] === 0xff) return `::ffff:${b.slice(12).join('.')}`;
  const groups = [];
  for (let i = 0; i < 16; i += 2) groups.push((b[i] << 8) | b[i + 1]);
  let best = [-1, 0];
  for (let i = 0; i < 8;) {
    if (groups[i] !== 0) { i++; continue; }
    let j = i;
    while (j < 8 && groups[j] === 0) j++;
    if (j - i > best[1] && j - i > 1) best = [i, j - i];
    i = j;
  }
  const hex = groups.map(g => g.toString(16));
  if (best[0] < 0) return hex.join(':');
  return `${hex.slice(0, best[0]).join(':')}::${hex.slice(best[0] + best[1]).join(':')}`;
}

/** "0100007F:1F90" → {address: '127.0.0.1', port: 8080}. */
export function parseEndpoint(text) {
  const [hex, port] = text.split(':');
  const b = bytes(hex);
  return {address: b.length === 4 ? b.join('.') : formatIPv6(b), port: parseInt(port, 16)};
}

/**
 * Listening sockets from one /proc/net table: TCP sockets in LISTEN and
 * unconnected UDP sockets. `proto` is 'tcp' or 'udp'.
 */
export function parseTable(text, proto) {
  const sockets = [];
  for (const line of String(text).split('\n').slice(1)) {
    const f = line.trim().split(/\s+/);
    if (f.length < 10) continue;
    const local = parseEndpoint(f[1]);
    const remote = parseEndpoint(f[2]);
    const listening = proto === 'tcp' ? f[3] === TCP_LISTEN : f[3] === UDP_UNCONNECTED && remote.port === 0;
    if (!listening || local.port === 0) continue;
    sockets.push({proto, ...local, uid: Number(f[7]), inode: f[9]});
  }
  return sockets;
}

/** "socket:[1234]" → "1234". */
export function socketInode(link) {
  return /^socket:\[(\d+)\]$/.exec(link)?.[1] ?? null;
}

/** uid → user name from /etc/passwd text. */
export function parsePasswd(text) {
  const users = new Map();
  for (const line of String(text).split('\n')) {
    const [name, , uid] = line.split(':');
    if (name && uid) users.set(Number(uid), name);
  }
  return users;
}

/**
 * One row per program and port: a server bound to both 0.0.0.0 and :: shows
 * once with both addresses. TCP sorts before UDP, then by port.
 */
export function group(sockets, owners) {
  const rows = new Map();
  for (const socket of sockets) {
    const pid = owners.get(socket.inode) ?? null;
    const key = `${socket.proto}:${socket.port}:${pid ?? `uid${socket.uid}`}`;
    const row = rows.get(key) ?? {id: key, proto: socket.proto, port: socket.port, pid, uid: socket.uid, addresses: []};
    if (!row.addresses.includes(socket.address)) row.addresses.push(socket.address);
    rows.set(key, row);
  }
  return [...rows.values()].sort((a, b) => (a.proto === b.proto ? 0 : a.proto === 'tcp' ? -1 : 1) || a.port - b.port);
}

/** "3000 api" or "api 3000" → {port: 3000, name: 'api'}. */
export function parseNaming(text) {
  const match = /(?:^|\s)(\d{1,5})(?=\s|$)/.exec(String(text ?? ''));
  if (!match) return null;
  const port = Number(match[1]);
  if (port < 1 || port > 65535) return null;
  const name = (text.slice(0, match.index) + ' ' + text.slice(match.index + match[0].length)).replace(/\s+/g, ' ').trim();
  return {port, name};
}

/** How reachable a listening socket is from outside this computer, judged by its bind address. */
export function exposure(address) {
  if (address === '0.0.0.0' || address === '::') return 'network';
  if (address === '::1' || address.startsWith('127.') || address.startsWith('::ffff:127.')) return 'local';
  return 'specific';
}
export const EXPOSURE = {
  network: {label: 'Network', color: 'orange', description: 'Reachable from other computers on the network'},
  local: {label: 'This computer', color: 'green', description: 'Reachable only from this computer'},
  specific: {label: 'One interface', color: 'blue', description: 'Bound to one network address'},
};
/** The widest exposure among several addresses: network beats one interface beats local. */
export function widest(addresses) {
  const kinds = addresses.map(exposure);
  return kinds.includes('network') ? 'network' : kinds.includes('specific') ? 'specific' : 'local';
}

/**
 * One entry per program: every port a known PID listens on, sorted. Sockets
 * whose program is hidden (another user's) stay one entry per port and user.
 */
export function byProgram(rows) {
  const programs = new Map();
  for (const row of rows) {
    // A hidden program's TCP and UDP sockets on one port are almost always the same service.
    const key = row.pid ? `pid:${row.pid}` : `port:${row.port}:${row.uid}`;
    const program = programs.get(key) ?? {id: key, pid: row.pid, uid: row.uid, ports: []};
    program.ports.push({proto: row.proto, port: row.port, addresses: row.addresses});
    programs.set(key, program);
  }
  for (const program of programs.values()) {
    program.ports.sort((a, b) => a.port - b.port || a.proto.localeCompare(b.proto));
  }
  return [...programs.values()].sort((a, b) => a.ports[0].port - b.ports[0].port);
}
