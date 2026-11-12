#!/usr/bin/env node
import {build} from 'esbuild';
import {readFile, mkdir, writeFile} from 'node:fs/promises';
import {resolve, relative, isAbsolute} from 'node:path';

const [command, input, output] = process.argv.slice(2);
if (command === 'build' && input && output) {
  const root = resolve(input); const destination = resolve(output);
  const offset = relative(root, destination);
  if (!offset || (!offset.startsWith('..') && !isAbsolute(offset))) throw new Error('Build output must be outside the project directory');
  const manifest = JSON.parse(await readFile(resolve(root, 'extension.json'), 'utf8'));
  await mkdir(destination, {recursive: true});
  await build({entryPoints: [resolve(root, manifest.entry)], outfile: resolve(destination, 'index.mjs'), bundle: true, platform: 'node', target: 'node22', format: 'esm'});
  await writeFile(resolve(destination, 'extension.json'), `${JSON.stringify({...manifest, entry: 'index.mjs'}, null, 2)}\n`);
  console.log(`Built ${manifest.name} in ${destination}. Package with: gnome-launcher pack "${destination}" OUTPUT.lumen-extension`);
} else if (command === 'create' && input) {
  const destination = resolve(input); await mkdir(destination, {recursive: false});
  const id = destination.split('/').at(-1).toLowerCase().replace(/[^a-z0-9-]/g, '-');
  await writeFile(resolve(destination, 'extension.json'), `${JSON.stringify({id, name: 'My Extension', version: '1.0.0', sdk_version: 1, entry: 'index.mjs', capabilities: [], commands: [{id: 'hello', title: 'Hello from my extension'}]}, null, 2)}\n`);
  await writeFile(resolve(destination, 'package.json'), `${JSON.stringify({name: id, private: true, type: 'module', dependencies: {'@lumen/extension-sdk': 'file:' + resolve(import.meta.dirname)}}, null, 2)}\n`);
  await writeFile(resolve(destination, 'index.mjs'), "import {serve, detail} from '@lumen/extension-sdk';\nserve({hello: {run: () => detail('Hello', 'Your extension is ready.')}});\n");
  console.log('Created project. Run npm install inside it, then use lumen-extension build PROJECT OUTPUT.');
} else {
  console.log('lumen-extension create FOLDER\nlumen-extension build PROJECT OUTPUT\nSupports JS and TS entry points. Bundle before packaging; no install scripts run in Lumen.');
  process.exitCode = command ? 1 : 0;
}
