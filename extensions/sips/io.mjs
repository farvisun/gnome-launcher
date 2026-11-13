// Where images come from (the Files selection, the clipboard or a chooser)
// and where results go, following the extension's preferences.
import {execFileSync} from 'node:child_process';
import {existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, statSync} from 'node:fs';
import {homedir} from 'node:os';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {host} from '../../sdk/index.mjs';
import {destinations, isImage} from './lib.mjs';

const runtime = process.env.XDG_RUNTIME_DIR || `/run/user/${process.getuid()}`;
const cache = path.join(process.env.XDG_CACHE_HOME || path.join(homedir(), '.cache'), 'gnome-launcher', 'lumen.sips');
export const SELECTION = path.join(runtime, 'gnome-launcher', 'nautilus-selection.json');
export const CACHE = cache;
const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/gif', 'image/bmp', 'image/tiff', 'image/webp', 'image/avif',
  'image/heic', 'image/heif', 'image/svg+xml', 'image/x-icon', 'image/vnd.microsoft.icon', 'image/x-tga', 'image/jp2',
  'image/x-portable-bitmap', 'image/vnd.adobe.photoshop', 'image/x-exr', 'image/jxl'];

/**
 * What Files last reported, written by the Lumen nautilus-python helper on every
 * selection change, or null when Files is not running.
 */
export function filesSelection() {
  try {
    const state = JSON.parse(readFileSync(SELECTION, 'utf8'));
    const comm = readFileSync(`/proc/${Number(state.pid)}/comm`, 'utf8').trim();
    if (!comm.startsWith('nautilus')) return null;
    return {files: (state.files ?? []).filter(f => typeof f === 'string' && existsSync(f)), folder: state.folder ?? null};
  } catch {
    return null;
  }
}

/** XDG user directories, such as DESKTOP or PICTURES. */
export function userDir(name, fallback) {
  try {
    const dir = execFileSync('xdg-user-dir', [name], {encoding: 'utf8'}).trim();
    if (dir && dir !== homedir()) return dir;
  } catch { /* not installed */ }
  return path.join(homedir(), fallback);
}

/**
 * The images to work on. `interactive` false never opens a chooser (for previews).
 * Returns {files, fromClipboard, folder}.
 */
export async function selectedImages({preferences, launch_context}, {interactive = true} = {}) {
  if (Array.isArray(launch_context?.files) && launch_context.files.length) {
    const files = launch_context.files.filter(f => typeof f === 'string' && existsSync(f) && isImage(f));
    return {files, fromClipboard: false, folder: path.dirname(files[0] ?? homedir())};
  }
  const selection = filesSelection();
  if (preferences.inputMethod === 'Clipboard') {
    const clip = await host.readClipboard();
    if (clip.files?.length) return {files: clip.files.filter(isImage), fromClipboard: false, folder: selection?.folder};
    if (clip.image) return {files: [clip.image], fromClipboard: true, folder: selection?.folder};
  } else if (selection?.files.length) {
    return {files: selection.files.filter(isImage), fromClipboard: false, folder: selection.folder};
  }
  if (!interactive) return {files: [], fromClipboard: false, folder: selection?.folder};
  const chosen = await host.chooseFiles({title: 'Choose Images', mimeTypes: IMAGE_TYPES, filterName: 'Images'});
  return {files: chosen.files.filter(isImage), fromClipboard: false, folder: selection?.folder};
}

/** Where results go for these sources, per the imageResultHandling preference. */
export function plan(sources, {preferences, fromClipboard = false, generated = false, folder, extension, suffix}) {
  const scratch = path.join(cache, 'results');
  mkdirSync(scratch, {recursive: true});
  prune(scratch);
  const handling = fromClipboard && preferences.imageResultHandling === 'replaceOriginal' ? 'copyToClipboard' : preferences.imageResultHandling;
  return destinations(sources, {
    handling, fromClipboard, generated, extension, suffix, scratch,
    folder: folder && existsSync(folder) ? folder : userDir('PICTURES', 'Pictures'),
    downloads: userDir('DOWNLOAD', 'Downloads'),
    desktop: userDir('DESKTOP', 'Desktop'),
  }, existsSync);
}

/** Results waiting for the clipboard or a viewer are kept a day. */
function prune(dir) {
  const old = Date.now() - 24 * 3600_000;
  for (const name of readdirSync(dir)) {
    const file = path.join(dir, name);
    try { if (statSync(file).mtimeMs < old) rmSync(file, {force: true}); } catch { /* gone */ }
  }
}

/**
 * Writes through a temporary file beside the target, so a failed command never
 * leaves half an image where the original was.
 */
export async function writeAtomically(target, write) {
  mkdirSync(path.dirname(target), {recursive: true});
  const ext = path.extname(target);
  const temporary = path.join(path.dirname(target), `.${path.basename(target, ext)}.lumen-${process.pid}${ext}`);
  try {
    await write(temporary);
    renameSync(temporary, target);
  } finally {
    rmSync(temporary, {force: true});
  }
}

/** Copies results to the clipboard or opens them, as the preference asks. */
export async function deliver(results, {preferences, fromClipboard = false}) {
  const handling = fromClipboard && preferences.imageResultHandling === 'replaceOriginal' ? 'copyToClipboard' : preferences.imageResultHandling;
  if (handling === 'copyToClipboard' && results.length) await host.copyFile(results.at(-1));
  if (handling === 'openInPreview') for (const file of results.slice(0, 10)) await host.open(pathToFileURL(file).href);
}

