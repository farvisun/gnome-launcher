import test from 'node:test';
import assert from 'node:assert/strict';
import {byProgram, exposure, formatIPv6, group, parseEndpoint, parseNaming, parsePasswd, parseTable, socketInode, widest} from './lib.mjs';

const TCP = `  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 111 1 0000000000000000 100 0 0 10 0
   1: 0100007F:9C40 0100007F:1F90 01 00000000:00000000 00:00000000 00000000  1000        0 112 1 0000000000000000 100 0 0 10 0
`;
const TCP6 = `  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 113 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000001000000:0277 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 114 1 0000000000000000 100 0 0 10 0
`;
const UDP = `   sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode ref pointer drops
  144: 00000000:14E9 00000000:0000 07 00000000:00000000 00:00000000 00000000   104        0 115 2 0000000000000000 0
  145: 0100007F:D431 0100007F:0035 01 00000000:00000000 00:00000000 00000000  1000        0 116 2 0000000000000000 0
`;

test('decodes kernel endpoints', () => {
  assert.deepEqual(parseEndpoint('0100007F:1F90'), {address: '127.0.0.1', port: 8080});
  assert.deepEqual(parseEndpoint('00000000000000000000000001000000:0277'), {address: '::1', port: 631});
  assert.deepEqual(parseEndpoint('0000000000000000FFFF00000100007F:0050'), {address: '::ffff:127.0.0.1', port: 80});
  assert.equal(formatIPv6([0x20, 1, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]), '2001:db8::1');
});
test('keeps only listening TCP and unconnected UDP sockets', () => {
  assert.deepEqual(parseTable(TCP, 'tcp').map(s => s.port), [8080]);
  assert.deepEqual(parseTable(TCP6, 'tcp').map(s => [s.address, s.port, s.uid]), [['::', 8080, 1000], ['::1', 631, 0]]);
  assert.deepEqual(parseTable(UDP, 'udp').map(s => [s.port, s.uid]), [[5353, 104]]);
});
test('merges one program bound to several addresses', () => {
  const sockets = [...parseTable(TCP, 'tcp'), ...parseTable(TCP6, 'tcp'), ...parseTable(UDP, 'udp')];
  const rows = group(sockets, new Map([['111', 42], ['113', 42]]));
  assert.deepEqual(rows.map(r => [r.proto, r.port, r.pid, r.addresses]), [
    ['tcp', 631, null, ['::1']], ['tcp', 8080, 42, ['127.0.0.1', '::']], ['udp', 5353, null, ['0.0.0.0']],
  ]);
});
test('parses helpers', () => {
  assert.equal(socketInode('socket:[123]'), '123');
  assert.equal(socketInode('pipe:[123]'), null);
  assert.equal(parsePasswd('root:x:0:0::/root:/bin/sh\nme:x:1000:1000::/home/me:/bin/zsh').get(1000), 'me');
  assert.deepEqual(parseNaming('3000 API server'), {port: 3000, name: 'API server'});
  assert.deepEqual(parseNaming('db 5432'), {port: 5432, name: 'db'});
  assert.equal(parseNaming('70000 x'), null);
});
test('classifies exposure and groups ports by program', () => {
  assert.equal(exposure('0.0.0.0'), 'network');
  assert.equal(exposure('::'), 'network');
  assert.equal(exposure('127.0.0.53'), 'local');
  assert.equal(exposure('::1'), 'local');
  assert.equal(exposure('192.168.1.5'), 'specific');
  assert.equal(widest(['127.0.0.1', '0.0.0.0']), 'network');
  assert.equal(widest(['127.0.0.1', '10.0.0.2']), 'specific');
  const programs = byProgram([
    {proto: 'tcp', port: 9090, pid: 7, uid: 1000, addresses: ['127.0.0.1']},
    {proto: 'tcp', port: 631, pid: null, uid: 0, addresses: ['::1']},
    {proto: 'udp', port: 5533, pid: 7, uid: 1000, addresses: ['127.0.0.1']},
    {proto: 'tcp', port: 5533, pid: 7, uid: 1000, addresses: ['127.0.0.1']},
  ]);
  assert.deepEqual(programs.map(p => [p.id, p.ports.map(x => `${x.port}/${x.proto}`)]), [
    ['port:631:0', ['631/tcp']], ['pid:7', ['5533/tcp', '5533/udp', '9090/tcp']],
  ]);
});
