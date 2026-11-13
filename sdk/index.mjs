import {createInterface} from 'node:readline';
import {AsyncLocalStorage} from 'node:async_hooks';

const scope = new AsyncLocalStorage();
const pending = new Map();
let serial = 0;
let send = value => process.stdout.write(`${JSON.stringify(value)}\n`);

function call(method, params) {
  const request = scope.getStore();
  if (!request) return Promise.reject(new Error('Host calls require an active command request'));
  if (request.signal.aborted) return Promise.reject(new Error('Cancelled'));
  const id = `host:${++serial}`;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('Host request timed out')); }, 8000);
    pending.set(id, {resolve, reject, timer});
    send({jsonrpc: '2.0', id, method, params});
  });
}
export const host = Object.freeze({
  copy: text => call('host.copy', {text}),
  open: text => call('host.open', {text}),
  notify: text => call('host.notify', {text}),
  storage: Object.freeze({get: key => call('host.storage.get', {key}), set: (key, value) => call('host.storage.set', {key, value})}),
});
export function update(view) {
  const request = scope.getStore();
  if (!request || request.signal.aborted) return;
  send({jsonrpc: '2.0', method: 'view.update', params: {request_id: request.id, view}});
}
/** `options` is a title string, or {title, placeholder, detail, refresh, empty}. */
export function list(items, options) {
  return typeof options === 'object' && options !== null ? {type: 'list', items, ...options} : {type: 'list', items, title: options};
}
export function detail(title, text) { return {type: 'detail', title, text}; }
export function form(title, fields, submit_label = 'Submit') { return {type: 'form', title, fields, submit_label}; }

export function serve(commands) {
  // Console output must never corrupt the protocol stream.
  console.log = console.info = console.debug = (...args) => console.error(...args);
  const running = new Map();
  const input = createInterface({input: process.stdin, crlfDelay: Infinity});
  input.on('line', async line => {
    if (Buffer.byteLength(line) > 1024 * 1024) { console.error('Protocol frame too large'); process.exit(1); }
    let message;
    try { message = JSON.parse(line); } catch { console.error('Invalid protocol JSON'); return; }
    if (message.jsonrpc !== '2.0') return;
    if (!message.method) {
      const waiting = pending.get(message.id);
      if (!waiting) return;
      pending.delete(message.id); clearTimeout(waiting.timer);
      if (message.error) waiting.reject(new Error(message.error.message)); else waiting.resolve(message.result);
      return;
    }
    if (message.method === '$/cancelRequest') { running.get(message.params?.id)?.abort(); return; }
    const controller = new AbortController(); running.set(message.id, controller);
    try {
      const params = message.params ?? {};
      const command = commands[params.command];
      if (!command) throw new Error(`Unknown command: ${params.command}`);
      const handler = { 'command.run': command.run, 'command.query': command.query ?? command.run, 'command.action': command.action, 'command.submit': command.submit }[message.method];
      if (!handler) throw new Error(`Unsupported command method: ${message.method}`);
      const result = await scope.run({id: message.id, signal: controller.signal}, () => handler({...params, signal: controller.signal}));
      if (!controller.signal.aborted) send({jsonrpc: '2.0', id: message.id, result: result ?? null});
    } catch (error) {
      if (!controller.signal.aborted) send({jsonrpc: '2.0', id: message.id, error: {code: -32000, message: error instanceof Error ? error.message : String(error)}});
    } finally { running.delete(message.id); }
  });
  input.on('close', () => { for (const controller of running.values()) controller.abort(); process.exit(0); });
}
