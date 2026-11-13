// Pure helpers for Image Modification: argument parsing, colours and where
// results go. Ported from Raycast's "Image Modification" (sips) extension.
import path from 'node:path';

/** Evaluates arithmetic such as `90*2` or `pi/2`, like the mathjs parser Raycast uses. */
export function evaluate(text) {
  const source = String(text ?? '').trim().toLowerCase();
  let at = 0;
  const peek = () => source[at];
  const skip = () => { while (peek() === ' ') at++; };
  const fail = () => { throw new Error(`Cannot read “${text}” as a number`); };
  function primary() {
    skip();
    if (peek() === '(') { at++; const value = sum(); skip(); if (peek() !== ')') fail(); at++; return value; }
    if (peek() === '-') { at++; return -primary(); }
    if (peek() === '+') { at++; return primary(); }
    if (source.startsWith('pi', at)) { at += 2; return Math.PI; }
    if (peek() === 'e' && !/[a-z]/.test(source[at + 1] ?? '')) { at++; return Math.E; }
    const match = /^\d*\.?\d+(e[-+]?\d+)?/.exec(source.slice(at));
    if (!match) fail();
    at += match[0].length;
    return Number(match[0]);
  }
  function power() {
    const base = primary(); skip();
    if (peek() === '^') { at++; return base ** power(); }
    return base;
  }
  function product() {
    let value = power();
    for (;;) {
      skip();
      if (peek() === '*') { at++; value *= power(); } else if (peek() === '/') { at++; value /= power(); } else if (peek() === '%') { at++; value %= power(); } else return value;
    }
  }
  function sum() {
    let value = product();
    for (;;) {
      skip();
      if (peek() === '+') { at++; value += product(); } else if (peek() === '-') { at++; value -= product(); } else return value;
    }
  }
  if (!source) fail();
  const value = sum();
  skip();
  if (at !== source.length || !Number.isFinite(value)) fail();
  return value;
}

const NAMED = new Set(['black', 'white', 'red', 'green', 'blue', 'yellow', 'orange', 'purple', 'pink', 'gray', 'grey',
  'cyan', 'magenta', 'brown', 'navy', 'teal', 'olive', 'maroon', 'lime', 'aqua', 'silver', 'gold', 'transparent', 'none']);
/** A hex colour (#RGB, #RRGGBB, #RRGGBBAA, with or without #) or a common colour name, for ImageMagick. */
export function parseColor(text) {
  const value = String(text ?? '').trim();
  if (!value) return null;
  const hex = value.replace(/^#/, '');
  if (/^([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(hex)) return `#${hex.toUpperCase()}`;
  if (NAMED.has(value.toLowerCase())) return value.toLowerCase();
  return null;
}

export const pluralize = (count, one = 'image', many = `${one}s`) => `${count} ${count === 1 ? one : many}`;

export const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'jpe', 'gif', 'bmp', 'tif', 'tiff', 'webp', 'avif', 'heic', 'heif',
  'ico', 'icns', 'tga', 'psd', 'jp2', 'j2k', 'exr', 'dds', 'pbm', 'pgm', 'ppm', 'pnm', 'svg', 'jxl', 'qoi']);
export const isImage = file => IMAGE_EXTENSIONS.has(path.extname(file).slice(1).toLowerCase());

/**
 * Where each result goes, as Raycast's getDestinationPaths decides:
 * - `handling`: the imageResultHandling preference;
 * - `fromClipboard`/`generated`: no original folder, so "replace" and "containing
 *   folder" use `folder` (the open Files folder, or Pictures);
 * - `scratch`: where results wait before going to the clipboard or a viewer;
 * - `extension`: a new extension, `suffix`: added to the name ("-optimized").
 * Anything but "replace" never overwrites: names gain -2, -3…
 */
export function destinations(sources, options, exists) {
  const {handling, fromClipboard = false, generated = false, folder, downloads, desktop, scratch, extension, suffix = ''} = options;
  const taken = new Set();
  return sources.map(source => {
    const ext = extension ? `.${extension}` : path.extname(source);
    const name = path.basename(source, path.extname(source)) + suffix + ext;
    let dir = path.dirname(source);
    if (handling === 'saveToDownloads') dir = downloads;
    else if (handling === 'saveToDesktop') dir = desktop;
    else if (handling === 'copyToClipboard' || handling === 'openInPreview') dir = scratch;
    else if (fromClipboard || generated) dir = folder;
    let target = path.join(dir, name);
    // Only the original itself is replaced; a new extension never overwrites another file.
    const replacing = handling === 'replaceOriginal' && !fromClipboard && !generated && target === source;
    if (!replacing && dir !== scratch) {
      for (let n = 2; exists(target) || taken.has(target); n++) {
        target = path.join(dir, `${path.basename(name, ext)}-${n}${ext}`);
      }
    }
    taken.add(target);
    return target;
  });
}

/** Raycast's convert list, with ImageMagick's coder names. */
export const FORMATS = [
  ['ASTC', 'ASTC', 'astc'], ['AVIF', 'AVIF', 'avif'], ['BMP', 'BMP', 'bmp'], ['DDS', 'DDS', 'dds'],
  ['EXR', 'EXR', 'exr'], ['GIF', 'GIF', 'gif'], ['HEIC', 'HEIC', 'heic'], ['HEICS', 'HEICS', 'heics'],
  ['ICNS', 'ICNS', 'icns'], ['ICO', 'ICO', 'ico'], ['JPEG', 'JPEG', 'jpeg'], ['JP2', 'JP2', 'jp2'],
  ['KTX', 'KTX', 'ktx'], ['PBM', 'PBM', 'pbm'], ['PDF', 'PDF', 'pdf'], ['PNG', 'PNG', 'png'],
  ['PSD', 'PSD', 'psd'], ['PVR', 'PVR', 'pvr'], ['TGA', 'TGA', 'tga'], ['TIFF', 'TIFF', 'tiff'],
  ['WEBP', 'WEBP', 'webp'], ['SVG', 'SVG', 'svg'],
].map(([name, coder, extension]) => ({name, coder, extension}));

/** Which formats Convert offers: writable here, and not hidden in the preferences. */
export function convertFormats(writable, preferences, {potrace = false} = {}) {
  return FORMATS.filter(f => preferences[`show${f.name}`] !== false)
    .filter(f => (f.name === 'SVG' ? potrace : writable.has(f.coder)))
    .map(f => (f.name === 'JPEG' && preferences.jpegExtension === 'jpg' ? {...f, extension: 'jpg'} : f));
}

/** Parses `magick -list format`: the coders that can write. */
export function writableFormats(listing) {
  const writable = new Set();
  for (const line of String(listing).split('\n')) {
    const match = /^\s*([A-Z0-9-]+)\*?\s+\S+\s+([r-])([w-])([+-])/.exec(line);
    if (match && match[3] === 'w') writable.add(match[1]);
  }
  return writable;
}
