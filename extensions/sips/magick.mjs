// Runs ImageMagick 7 (`magick`), the Linux stand-in for macOS `sips` and Core Image.
import {execFile} from 'node:child_process';
import {existsSync, mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {writableFormats} from './lib.mjs';

export const INSTALL_HINT = 'Install ImageMagick: sudo apt install imagemagick';

/** The first of these programs on PATH. */
export function which(...names) {
  for (const name of names) {
    if (name.includes('/')) { if (existsSync(name)) return name; continue; }
    for (const dir of (process.env.PATH ?? '').split(':').filter(Boolean)) {
      const candidate = path.join(dir, name);
      if (existsSync(candidate)) return candidate;
    }
  }
  return null;
}

let binary;
/** `magick`, as LUMEN_MAGICK overrides for development. */
export function magickPath() {
  binary ??= which(...[process.env.LUMEN_MAGICK, 'magick', 'magick-im7.q16', 'magick-im7.q16hdri'].filter(Boolean));
  return binary;
}

export class MissingTool extends Error {}

/** Runs a program; rejects with its error output. */
export function run(file, args, {signal, input} = {}) {
  return new Promise((resolve, reject) => {
    const child = execFile(file, args, {signal, maxBuffer: 64 * 1024 * 1024, encoding: 'buffer'}, (error, stdout, stderr) => {
      if (error) {
        const message = stderr.toString().trim().split('\n').filter(l => !/^\s*$/.test(l)).slice(-2).join(' ');
        reject(new Error(message || error.message));
      } else resolve(stdout);
    });
    if (input) child.stdin.end(input);
  });
}

export async function magick(args, options) {
  const file = magickPath();
  if (!file) throw new MissingTool(INSTALL_HINT);
  return run(file, args, options);
}

/** Width and height of the first frame. */
export async function size(file, signal) {
  const out = (await magick(['identify', '-format', '%w %h\n', `${file}[0]`], {signal})).toString();
  const [width, height] = out.trim().split(/\s+/).map(Number);
  return {width, height};
}

let formats;
export async function writable() {
  formats ??= writableFormats((await magick(['-list', 'format'])).toString());
  return formats;
}

let pdf;
/** Ubuntu's ImageMagick policy blocks PDF by default; check once by trying. */
export async function canWritePdf() {
  if (pdf !== undefined) return pdf;
  const dir = mkdtempSync(path.join(tmpdir(), 'lumen-pdf-'));
  try {
    await magick(['-size', '2x2', 'xc:white', path.join(dir, 'probe.pdf')]);
    pdf = true;
  } catch {
    pdf = false;
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
  return pdf;
}
