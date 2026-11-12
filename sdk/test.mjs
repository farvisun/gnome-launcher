import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
import {once} from 'node:events';
import {list, detail, form} from './index.mjs';

test('view helpers produce the public wire format', () => {
  assert.deepEqual(list([{id:'one',title:'One'}]).items, [{id:'one',title:'One'}]);
  assert.equal(detail('Title','Body').text, 'Body');
  assert.equal(form('Title', []).submit_label, 'Submit');
});
test('SDK serves commands, host actions, and errors without corrupting stdout', async () => {
  const child = spawn(process.execPath, ['examples/quick-links/index.mjs'], {stdio: ['pipe','pipe','pipe']});
  const input = createInterface({input: child.stdout}); const lines = input[Symbol.asyncIterator]();
  const send = value => child.stdin.write(`${JSON.stringify({jsonrpc:'2.0',...value})}\n`);
  try {
    send({id:1,method:'command.run',params:{command:'links',query:'gnome'}});
    const result = JSON.parse((await lines.next()).value);
    assert.equal(result.id,1); assert.equal(result.result.type,'list'); assert.equal(result.result.items.length,3);
    send({id:2,method:'command.action',params:{command:'links',item:'gnome',action:'copy'}});
    const host = JSON.parse((await lines.next()).value); assert.equal(host.method,'host.copy'); assert.equal(host.params.text,'https://www.gnome.org/');
    send({id:host.id,result:null});
    assert.equal(JSON.parse((await lines.next()).value).id,2);
    send({id:3,method:'command.run',params:{command:'missing'}});
    assert.match(JSON.parse((await lines.next()).value).error.message,/Unknown command/);
  } finally { input.close(); child.kill(); await once(child,'exit'); }
});
