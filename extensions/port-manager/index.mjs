import {execFile} from 'node:child_process';
import {readFileSync, readdirSync, readlinkSync} from 'node:fs';
import {serve, list, form, update, host} from '../../sdk/index.mjs';
import {byProgram, group, parsePasswd, parseNaming, parseTable, socketInode, widest} from './lib.mjs';
import {portsList, programTitle, searchText} from './views.mjs';

const NAMES = 'names';
const read = path => { try { return readFileSync(path, 'utf8'); } catch { return ''; } };
const link = path => { try { return readlinkSync(path); } catch { return null; } };

/** Socket inode → PID. Only this user's processes can be inspected without root. */
function socketOwners(wanted) {
  const owners = new Map();
  for (const pid of readdirSync('/proc').filter(name => /^\d+$/.test(name))) {
    let fds;
    try { fds = readdirSync(`/proc/${pid}/fd`); } catch { continue; }
    for (const fd of fds) {
      const inode = socketInode(link(`/proc/${pid}/fd/${fd}`) ?? '');
      if (inode && wanted.has(inode) && !owners.has(inode)) owners.set(inode, Number(pid));
    }
  }
  return owners;
}

async function names() {
  const value = await host.storage.get(NAMES);
  return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

/** Every program listening on a port, with what /proc tells about it. */
async function programs() {
  // UDP sockets in the ephemeral range are almost always clients (DNS lookups,
  // discovery), not servers, so they would only bury the real listeners.
  const [low, high] = read('/proc/sys/net/ipv4/ip_local_port_range').trim().split(/\s+/).map(Number);
  const sockets = ['tcp', 'tcp6', 'udp', 'udp6']
    .flatMap(table => parseTable(read(`/proc/net/${table}`), table.slice(0, 3)))
    .filter(s => s.proto === 'tcp' || !(s.port >= low && s.port <= high));
  const owners = socketOwners(new Set(sockets.map(s => s.inode)));
  const users = parsePasswd(read('/etc/passwd'));
  const labels = await names();
  const me = process.getuid();
  return byProgram(group(sockets, owners)).map(program => {
    const {pid, uid} = program;
    // /proc/PID/stat: "pid (comm) state ppid …"; comm may contain spaces and parentheses.
    const stat = pid ? read(`/proc/${pid}/stat`) : '';
    const ppid = Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[1]) || null;
    return {
      ...program,
      mine: uid === me,
      user: uid === me ? 'you' : users.get(uid) ?? `uid ${uid}`,
      command: pid ? read(`/proc/${pid}/comm`).trim() || null : null,
      cmdline: pid ? read(`/proc/${pid}/cmdline`).split('\0').filter(Boolean).join(' ') : '',
      path: pid ? link(`/proc/${pid}/exe`) : null,
      ppid,
      parent: ppid ? read(`/proc/${ppid}/comm`).trim() || null : null,
      ports: program.ports.map(p => ({...p, name: labels[p.port] ?? null, exposure: widest(p.addresses)})),
    };
  });
}

async function portsView({query = ''}) {
  const needle = query.trim().toLowerCase();
  return portsList((await programs()).filter(p => !needle || searchText(p).includes(needle)), query);
}

function run(file, args, signal) {
  return new Promise((resolve, reject) => {
    execFile(file, args, {signal}, (error, _stdout, stderr) => {
      if (!error) return resolve();
      // pkexec: 126 means the dialog was dismissed, 127 that authorization failed.
      if (error.code === 126 || error.code === 127) return reject(new Error('Administrator approval was not given'));
      reject(new Error(stderr.trim() || error.message));
    });
  });
}

/** pkexec waits for the password dialog; repaint meanwhile so the request stays alive. */
async function asAdministrator(args, signal) {
  const waiting = list([{id: 'waiting', title: 'Waiting for administrator approval…', subtitle: 'Enter your password in the dialog to continue', icon: 'dialog-password-symbolic', tint: 'yellow'}], {placeholder: 'Waiting for approval…'});
  update(waiting);
  const keepAlive = setInterval(() => update(waiting), 4000);
  try {
    await run('pkexec', args, signal);
  } finally {
    clearInterval(keepAlive);
  }
}

async function stop(p, force, signal) {
  const signalName = force ? 'SIGKILL' : 'SIGTERM';
  const kill = () => asAdministrator(['kill', '-s', force ? 'KILL' : 'TERM', String(p.pid)], signal);
  if (p.pid && p.mine) {
    try {
      process.kill(p.pid, signalName);
    } catch (error) {
      if (error.code === 'ESRCH') return;
      if (error.code !== 'EPERM') throw error;
      await kill();
    }
  } else if (p.pid) {
    await kill();
  } else {
    // Without root the PID behind another user's socket is unknown; fuser finds it.
    await asAdministrator(['fuser', '-s', '-k', force ? '-KILL' : '-TERM', ...p.ports.map(x => `${x.port}/${x.proto}`)], signal);
  }
  // Give the program a moment to close its sockets before listing again.
  for (let i = 0; i < 10 && (await programs()).some(r => r.id === p.id); i++) await new Promise(r => setTimeout(r, 100));
}

function nameForm(port, name) {
  return form('Name a port', [
    {id: 'port', label: 'Port', type: 'text', required: true, value: port ? String(port) : '', placeholder: '3000'},
    {id: 'name', label: 'Name', type: 'text', value: name ?? '', placeholder: 'API server (leave empty to remove the name)'},
  ], 'Save');
}

async function saveName(values) {
  const port = Number(String(values.port ?? '').trim());
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('Enter a port between 1 and 65535');
  const labels = await names();
  const name = String(values.name ?? '').trim().slice(0, 40);
  if (name) labels[port] = name; else delete labels[port];
  await host.storage.set(NAMES, labels);
}

async function namesView({query = ''}) {
  const labels = await names();
  const listening = new Map();
  for (const p of await programs()) for (const x of p.ports) listening.set(String(x.port), programTitle(p));
  const items = [];
  const naming = parseNaming(query);
  if (naming?.name) {
    items.push({id: `set:${naming.port}`, section: 'New Name', title: `Name port ${naming.port} “${naming.name}”`, icon: 'list-add-symbolic', tint: 'green',
      accessories: listening.has(String(naming.port)) ? [{tag: `In use by ${listening.get(String(naming.port))}`, color: 'green'}] : [],
      actions: [{id: 'primary', title: 'Save name'}]});
  }
  const needle = naming?.name ? '' : query.trim().toLowerCase();
  for (const [port, name] of Object.entries(labels).sort((a, b) => a[0] - b[0])) {
    if (needle && !`${port} ${name}`.toLowerCase().includes(needle)) continue;
    const user = listening.get(port);
    items.push({id: `port:${port}`, section: 'Named Ports', title: name, subtitle: `Port ${port}`, icon: 'starred-symbolic', tint: 'accent',
      accessories: [user ? {tag: `In use · ${user}`, color: 'green'} : {tag: 'Not in use'}],
      actions: [{id: 'primary', title: 'Rename'}, {id: 'delete', title: 'Remove name'}]});
  }
  items.push({id: 'add', section: 'Add', title: 'Name a port', subtitle: 'Or type a port and a name, such as “3000 API server”', icon: 'list-add-symbolic', actions: [{id: 'primary', title: 'Name a port'}]});
  return list(items, {placeholder: 'Type a port and a name, such as 3000 API server'});
}

serve({
  'open-ports': {
    run: portsView,
    action: async ({item, action, query, signal}) => {
      const p = (await programs()).find(r => r.id === item);
      if (!p || action === 'reload') return portsView({query});
      switch (action) {
        case 'copy-port': await host.copy(p.ports.map(x => x.port).filter((v, i, a) => a.indexOf(v) === i).join(', ')); return null;
        case 'copy-pid': if (p.pid) await host.copy(String(p.pid)); return null;
        case 'copy-command': if (p.cmdline) await host.copy(p.cmdline); return null;
        case 'open': await host.open(`http://localhost:${p.ports.find(x => x.proto === 'tcp').port}`); return null;
        case 'name': return nameForm(p.ports[0].port, p.ports[0].name);
        default: await stop(p, action === 'force', signal); return portsView({query});
      }
    },
    submit: async ({values, query}) => { await saveName(values); return portsView({query}); },
  },
  'named-ports': {
    run: namesView,
    action: async ({item, action, query}) => {
      if (item === 'add') return nameForm(null, null);
      if (item.startsWith('set:')) {
        const naming = parseNaming(query);
        if (naming?.name) await saveName(naming);
        return namesView({query: ''});
      }
      const port = item.slice(5);
      const labels = await names();
      if (action === 'delete') {
        delete labels[port];
        await host.storage.set(NAMES, labels);
        return namesView({query});
      }
      return nameForm(port, labels[port]);
    },
    submit: async ({values}) => { await saveName(values); return namesView({query: ''}); },
  },
});
