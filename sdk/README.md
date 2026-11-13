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

Commands may set `"icon"` to a GNOME icon name (for example `"alarm-symbolic"`) to replace the default extension icon in search results.

Commands may also set:

- `"mode": "no-view"`: the command runs without opening a view, then Lumen closes. Report the outcome with `host.hud(text)` (shown by the Shell after Lumen closes) or a `failure` toast (keeps Lumen open).
- `"arguments"`: up to 3 `{name, placeholder, type?: "text" | "password" | "dropdown", required?, data?: [{title, value}]}`. They appear as small fields beside the search when the command is selected; Tab moves into them. Handlers receive them as `arguments`.
- `"preferences"`: settings for this command (see below).

### Preferences

The manifest's `preferences` (for every command) and each command's `preferences` are shown in Lumen's Settings under the extension, and handlers receive the merged values, with defaults applied, as `preferences`:

```json
"preferences": [
  {"name": "units", "type": "dropdown", "title": "Units", "default": "metric",
   "data": [{"title": "Metric", "value": "metric"}, {"title": "Imperial", "value": "imperial"}]},
  {"name": "compact", "type": "checkbox", "title": "Layout", "label": "Use a compact layout", "default": false},
  {"name": "token", "type": "password", "title": "API token", "required": true}
]
```

Types are `textfield`, `password`, `checkbox` and `dropdown`. If a `required` preference is empty, Lumen opens Settings instead of running the command. `host.openPreferences()` opens them from a command.

IDs contain lowercase ASCII letters, digits, dots, and hyphens. The `lumen.` prefix is reserved for the extensions that ship with Lumen. SDK version is the integer `1`. Commands have unique IDs. Entry points must resolve inside the project. Up to 50 commands are accepted. Packages permit regular files and directories only: no links, traversal paths, devices, or installation scripts. Limits: 20 MiB compressed, 50 MiB unpacked, 2,000 entries.

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

- `list(items, options?)`: up to 100 unique item IDs, title, optional subtitle/icon, and optional actions. Icons are GNOME icon names. Ctrl+K exposes additional actions, and the footer names the first action as what Enter does. `options` is a title string or an object:
  - `placeholder`: search field hint while the list shows.
  - `detail: true`: shows a side pane with the selected item's `detail`.
  - `refresh`: query the command again after this many milliseconds (500–60000) while the list stays open and idle, for live clocks.
  - `empty: {title, description?, icon?}`: what to show when `items` is empty.

  Items may also set:
  - `section`: consecutive items with the same section share a header.
  - `tint`: colour for the icon.
  - `progress`: 0–1, a thin bar under the subtitle.
  - `accessories`: up to 6 right-aligned `{text?, icon?, tag?, color?, tooltip?}`. A `tag` is drawn as a coloured pill.
  - `detail`: side-pane content, any of `hero: {value, unit?, caption?, progress?, color?}`, `chart: {values, caption?, color?}` (a sparkline of up to 200 numbers), `text`, and `metadata: [{label, text?, color?, tags?: [{text, color?}]} | {separator: true}]`.

  Colours are `red`, `orange`, `yellow`, `green`, `blue`, `purple`, `accent` and `dim`; they follow the GNOME palette in light and dark styles.
  List item icons may also be absolute paths of local images.
- `grid(items, options?)`: tiles with `content: {image}` (an absolute path to a local image) or `content: {icon, tint}`, plus `title`, `subtitle`, `section`, `tooltip` and `actions`. Options are those of lists plus `columns` (3–8, default 5) and `aspect` (`1`, `3/2`, `2/3`, `4/3`, `3/4`, `16/9`, `9/16`). Arrow keys move in two dimensions.
- `detail(title, text, {image?, metadata?})`: selectable plain text, up to 100 KB, with an optional local image and metadata rows. Arbitrary HTML and webviews are unsupported.
- `form(title, fields, submitLabel?)`: up to 30 fields, with unique IDs, labels, and `text`, `password`, `boolean`, `dropdown` (`options: [{value, title}]`) or `description` (read-only `text`) types. Optional `required`, `placeholder`, `value` and `info` (help text under the field). Enter in a text field or the button calls the command's `submit` handler.

Any view may also set:

- `push: true` (or wrap it in `push(view)`): show it over the current view, as Raycast's `Action.Push`. Esc returns to the previous view and its search text. Only `action` and `submit` results may push.
- `context`: any JSON up to 4 KB. Every later request from this view carries it back as `context`, so a pushed view can re-query itself.
- `selection: true` (lists and grids): after the selection rests for half a second, the command's `select({item})` handler runs, and may return a replacement view (keep the item IDs to keep the selection), as for live previews.

Host methods:

| Method | Capability | Behavior |
| --- | --- | --- |
| `await host.copy(text)` | clipboard | Copy text to the system clipboard |
| `await host.open(url)` | open | Open HTTP(S), file, or mailto URI with its registered app |
| `await host.notify(text)` | notifications | Show a desktop notification |
| `await host.storage.get(key)` | storage | Read JSON or return null |
| `await host.storage.set(key, value)` | storage | Persist JSON in this extension's namespace |
| `await host.toast({style, title, message?})` | — | Show `success`, `failure` or `animated` (with a spinner) in the status line |
| `await host.hud(text)` | — | Close Lumen when the request ends and show `text` as a HUD |
| `await host.closeWindow()` | — | Close Lumen when the request ends |
| `await host.openPreferences()` | — | Open this extension's preferences |
| `await host.readClipboard()` | clipboard | `{files}` copied in Files, `{image}` (a PNG path) for pixels, `{text}`, or `{}` |
| `await host.copyFile(path)` | clipboard | Copy a file, with its pixels when it is an image |
| `await host.chooseFiles({title?, mimeTypes?, multiple?})` | — | Open a file chooser; resolves to `{files}`, empty when cancelled |

Storage is limited to 1 MiB per extension. Host methods resolve after validation and queuing; UI effects are performed on the GTK thread. Report errors by throwing an `Error`. Write diagnostics with `console.error`; `serve` redirects console log/info/debug to stderr to protect stdout.

A request fails after ten seconds without any message from the worker; every `update(view)` or host call restarts that clock, so long tasks should send progress updates. No request may run longer than two minutes. Cancelled, crashed, or malformed workers are terminated; idle workers are reaped after 60–90 seconds. Persist anything needed across worker restarts using storage. Avoid background tasks after returning from a handler. Observe the abort signal in network requests and other asynchronous work.

## Wire protocol

The SDK uses newline-delimited JSON-RPC 2.0 over stdin/stdout. A developer can implement this protocol without the SDK. Stdout is reserved for protocol frames; stderr is for logs. Each frame is limited to 1 MiB and each request to 500 received frames.

Host requests use numeric IDs and methods `command.run`, `command.query`, `command.action`, `command.submit` and `command.select`. Params include `command`, `query`, `preferences`, `arguments`, `launch_context` and the visible view's `context`; actions also include `item` and `action`, selections include `item`, and submits include `values`. Responses use the same ID with a view in `result` or a JSON-RPC error.

Host sends `$/cancelRequest` with `{id}` before terminating a cancelled worker. For intermediate views, send `view.update` with `{request_id, view}`. Extension-to-host requests use string IDs (the SDK uses `host:1`, etc.) and methods `host.copy`, `host.open`, `host.notify` with `{text}`, or `host.storage.get`/`host.storage.set` with `{key, value?}`. Host responds with JSON result or an error.

See [Quick Links](../examples/quick-links/index.mjs) and [Scratchpad](../examples/scratchpad/index.mjs) for complete examples.
