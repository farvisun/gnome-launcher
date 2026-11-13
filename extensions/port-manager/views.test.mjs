import test from 'node:test';
import assert from 'node:assert/strict';
import {portsList, searchText} from './views.mjs';

const program = (id, mine, ports, extra = {}) => ({
  id, mine, pid: mine ? 100 : null, uid: mine ? 1000 : 0, user: mine ? 'you' : 'root', command: mine ? 'node' : null, cmdline: '',
  ports: ports.map(([port, address, name]) => ({proto: 'tcp', port, addresses: [address], name: name ?? null, exposure: address === '0.0.0.0' ? 'network' : 'local'})),
  ...extra,
});

test('puts your programs first and keeps at most two port tags per row', () => {
  const view = portsList([
    program('port:22:0', false, [[22, '0.0.0.0']]),
    program('pid:100', true, [[3000, '127.0.0.1', 'API'], [3001, '0.0.0.0'], [3002, '127.0.0.1']]),
  ]);
  assert.deepEqual(view.items.map(i => i.section), ['Your Programs', 'System and Other Users']);
  const mine = view.items[0];
  assert.deepEqual(mine.accessories.map(a => a.tag ?? a.text), ['3000 API', '3001', '+1']);
  assert.equal(mine.tint, 'orange');
  assert.equal(view.items[1].title, 'Port 22');
  assert.equal(view.refresh, 5000);
  assert.ok(searchText(program('x', true, [[3000, '0.0.0.0']])).includes('network'));
});
