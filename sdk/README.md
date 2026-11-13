# Lumen extension SDK v1

Node.js 22+ is required at runtime. JavaScript and TypeScript entry points are bundled into self-contained ESM by the development CLI. The runtime SDK has no external dependencies.

## Create and load

From the Lumen repository after `npm ci --ignore-scripts`:

```sh
node sdk/cli.mjs create /tmp/my-lumen-extension
cd /tmp/my-lumen-extension
npm install --ignore-scripts
```

The generated project references the local SDK by path. Build and package it (replace `/path/to/lumen` with this checkout):

```sh
node /path/to/lumen/sdk/cli.mjs build /tmp/my-lumen-extension /tmp/my-lumen-build
/path/to/lumen/target/release/gnome-launcher pack /tmp/my-lumen-build /tmp/my-extension.lumen-extension
```

Choose the package in Lumen's extension settings. To skip bundling during development, load your source directory from Settings. Restart a command after changing its code; disable/re-enable its extension to stop an existing worker immediately.

## Manifest

```json
{
  "id": "my-extension",
  "name": "My Extension",
  "version": "1.0.0",
  "sdk_version": 1,
  "entry": "index.mjs",
  "description": "A useful command",
  "capabilities": ["clipboard", "storage"],
  "commands": [{"id": "hello", "title": "Hello World", "keywords": ["example"]}]
}
```

IDs contain lowercase ASCII letters, digits, dots, and hyphens. SDK version is the integer `1`. Commands have unique IDs. Entry points must resolve inside the project. Up to 50 commands are accepted. Packages permit regular files and directories only: no links, traversal paths, devices, or installation scripts. Limits: 20 MiB compressed, 50 MiB unpacked, 2,000 entries.

Capability names are `clipboard`, `open`, `notifications`, `storage`, `network`, `filesystem`, and `process`. The first four are checked by host methods. The last three document an extension's direct Node access. These declarations **do not sandbox a program**.

## Commands and views

```js
import {serve, list, detail, form, update, host} from '@lumen/extension-sdk';

serve({hello: {
  run: ({query, signal}) => list([
    {id: 'greeting', title: 'Hello World', subtitle: 'An example',
     actions: [{id: 'primary', title: 'Copy greeting'}]}
  ]),
  query: ({query, signal}) => list([
    {id: 'greeting', title: `Hello ${query || 'World'}`}
  ]),
  action: async ({item, action, query, signal}) => {
    await host.copy(`Hello ${query || 'World'}`);
    return detail('Copied', 'The greeting is on your clipboard.');
  }
}});
```

`run` opens a command; `query` handles search changes and defaults to `run` if omitted. `action` receives the selected item ID and action ID (`primary` for Enter). `submit` receives a `values` object from a form. Every handler receives an `AbortSignal`. Return a view, `null`, or nothing; `null` keeps the current view. Use `update(view)` for intermediate views while the handler is running.

Native views:

- `list(items, title?)`: up to 100 unique item IDs, title, optional subtitle/icon, and optional actions. Icons are GNOME icon names. Ctrl+K exposes additional actions.
- `detail(title, text)`: selectable plain text, up to 100 KB. Arbitrary HTML and webviews are unsupported.
- `form(title, fields, submitLabel?)`: up to 30 fields, with unique IDs, labels, and `text`, `password`, or `boolean` types. Optional `required`, `placeholder`, and `value` attributes. Submission calls the command's `submit` handler.

Host methods:

| Method | Capability | Behavior |
| --- | --- | --- |
| `await host.copy(text)` | clipboard | Copy text to the system clipboard |
| `await host.open(url)` | open | Open HTTP(S), file, or mailto URI with its registered app |
| `await host.notify(text)` | notifications | Show a desktop notification |
| `await host.storage.get(key)` | storage | Read JSON or return null |
| `await host.storage.set(key, value)` | storage | Persist JSON in this extension's namespace |

Storage is limited to 1 MiB per extension. Host methods resolve after validation and queuing; UI effects are performed on the GTK thread. Report errors by throwing an `Error`. Write diagnostics with `console.error`; `serve` redirects console log/info/debug to stderr to protect stdout.

A request fails after ten seconds without any message from the worker; every `update(view)` or host call restarts that clock, so long tasks should send progress updates. No request may run longer than two minutes. Cancelled, crashed, or malformed workers are terminated; idle workers are reaped after 60–90 seconds. Persist anything needed across worker restarts using storage. Avoid background tasks after returning from a handler. Observe the abort signal in network requests and other asynchronous work.

## Wire protocol

The SDK uses newline-delimited JSON-RPC 2.0 over stdin/stdout. A developer can implement this protocol without the SDK. Stdout is reserved for protocol frames; stderr is for logs. Each frame is limited to 1 MiB and each request to 500 received frames.

Host requests use numeric IDs and methods `command.run`, `command.query`, `command.action`, and `command.submit`. Params include `command` and `query`; actions also include `item` and `action`, and submits include `values`. Responses use the same ID with a view in `result` or a JSON-RPC error.

Host sends `$/cancelRequest` with `{id}` before terminating a cancelled worker. For intermediate views, send `view.update` with `{request_id, view}`. Extension-to-host requests use string IDs (the SDK uses `host:1`, etc.) and methods `host.copy`, `host.open`, `host.notify` with `{text}`, or `host.storage.get`/`host.storage.set` with `{key, value?}`. Host responds with JSON result or an error.

See [Quick Links](../examples/quick-links/index.mjs) and [Scratchpad](../examples/scratchpad/index.mjs) for complete examples.
